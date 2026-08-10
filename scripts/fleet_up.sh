#!/usr/bin/env bash
#
# fleet_up.sh -- bring the ENTIRE oxidex fix fleet up with one command, supervised.
#
# WHY THIS EXISTS
# ---------------
# The pipeline already works end to end:
#
#     32 workers -> 14 squad mergers -> auto_publish_round -> origin/main
#                          |
#                          +-> ~/.oxidex/logs/quarantine.jsonl -> judgment queue
#
# What did NOT exist was a way to START it that a human could not get wrong.
# Every tier was hand-launched with `nohup ... &` from whatever shell happened
# to be open, and three separate outages traced back to exactly that:
#
#   1. CWD DECIDES WHICH PROGRAM RUNS. Every fleet script is invoked by
#      RELATIVE path (`uv run scripts/squad_merge_loop.py`). On 2026-07-26 the
#      byte-identical command line was run twice, 40 minutes apart, from two
#      different checkouts, and ran two different programs -- the second died
#      on a flag that did not exist in the older copy. This script resolves ONE
#      absolute, main-tracking checkout and invokes every tier by ABSOLUTE path.
#      That is the single most important thing in this file (see pin_repo).
#
#   2. NOTHING RESTARTED A DEAD TIER. On 2026-07-25 seven of fourteen mergers
#      died within seconds of each other on one unguarded CalledProcessError.
#      68% of worker slots had no publish path for over an hour before a human
#      noticed. scripts/supervise_mergers.sh was written in response, but it
#      only covers tier 2 and it defaults REPO to a STALE worktree (measured
#      2026-07-26: ~/.oxidex/worktrees/fleet-ops was at f2b5f9a1, not main) --
#      i.e. defect #1 is already checked into the repo.
#
#   3. LIVENESS WAS CHECKED WITH `pgrep -f`, WHICH SELF-MATCHES. Reproduced
#      2026-07-26: `pgrep -f squad_merge_loop` returned EIGHT pids for SEVEN
#      running mergers. The eighth was the invoking `/bin/zsh -c ...` whose own
#      argv contained the pattern. pgrep excludes its own pid but not an
#      ancestor shell, a nohup wrapper, or a sibling. A supervisor script whose
#      own command line mentions the tier it supervises therefore ALWAYS sees
#      that tier as alive -- which is precisely how a completely dead merger
#      tier looked healthy while nothing published for hours. Every liveness
#      check here is PID-VERIFIED against a pid we ourselves forked, and the
#      one scan that must search (foreign-fleet detection in preflight)
#      explicitly excludes our own pid and our whole ancestor chain.
#
# USAGE
#   ./scripts/fleet_up.sh                 # preflight, sync, start all tiers, supervise
#   ./scripts/fleet_up.sh --status        # exactly what is alive, from the pidfile
#   ./scripts/fleet_up.sh --down          # stop everything THIS launcher started
#   ./scripts/fleet_up.sh --workers 24    # dispatcher --max-parallel
#   ./scripts/fleet_up.sh --scale 10      # resize a RUNNING fleet: restarts only
#                                          # the dispatcher, leaves mergers and the
#                                          # judgment queue (and their batch state)
#                                          # alone. On a stopped fleet it records
#                                          # the size the next start will use.
#   ./scripts/fleet_up.sh --mergers 1     # only the first N config.toml squads get a merger
#                                          # (overrides config.toml's [fleet].mergers, if set)
#   ./scripts/fleet_up.sh --squad-mode     # allocate real per-squad worker slots
#   ./scripts/fleet_up.sh --dry-run       # preflight + plan only; mutates nothing
#   ./scripts/fleet_up.sh --no-judgment   # start WITHOUT the quarantine tier (see below)
#
# WHAT IT DELIBERATELY DOES NOT DO
#   * It does not kill processes it did not start. --down is pidfile-exact.
#     Reaping a fleet started by hand is stop_parallel_fix.py's job.
#   * It does not `git clean` worker worktrees. Untracked files there are
#     usually a half-written parser a human still wants; the reset is enough.
#   * It does not raise the cargo build cap. Both tiers already share
#     ~/.oxidex/logs/build-semaphore.json (max 5 holders); --workers only
#     changes how many workers queue behind it.

set -euo pipefail
# -E (errtrace) propagates the ERR trap into shell FUNCTIONS. Without it the
# trap installed in main() would not fire inside write_state/supervise/... --
# i.e. it would be absent from exactly the code that has actually killed this
# script. See on_err: on 2026-07-30 the supervisor died inside write_state and
# left no log line at all, because `set -e` exits silently by design.
set -E

# printf '%(...)T' (used once per log line) is bash >= 4.2, and this script is
# the thing that is supposed to fail LOUDLY rather than half-work. macOS ships
# bash 3.2 at /bin/bash; `env bash` picks up a modern one from PATH.
if [ -z "${BASH_VERSINFO:-}" ] || [ "${BASH_VERSINFO[0]}" -lt 4 ]; then
    echo "fleet_up.sh: needs bash >= 4 (found ${BASH_VERSION:-unknown})." >&2
    echo "  remedy: brew install bash, then re-run via /usr/bin/env bash" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Configuration. Every one of these is env-overridable so the unit tests in
# scripts/test_fleet_up.py can point the whole script at a tempdir.
# ---------------------------------------------------------------------------

# OXIDEX_HOME is read AT IMPORT TIME by find_tag_gaps.py and frozen into ~8
# constants (log dir, dispatcher lock, worktree dirs, build semaphore). If the
# tiers disagree about it they stop seeing each other's state entirely, so it
# is resolved ONCE here and exported to every child.
OXIDEX_HOME="${OXIDEX_HOME:-$HOME/.oxidex}"
export OXIDEX_HOME

FLEET_LOG_DIR="${FLEET_LOG_DIR:-$OXIDEX_HOME/logs}"
FLEET_LOG="${FLEET_LOG:-$FLEET_LOG_DIR/fleet-up.log}"
FLEET_PIDFILE="${FLEET_PIDFILE:-$FLEET_LOG_DIR/fleet-up.pid}"
FLEET_STATEFILE="${FLEET_STATEFILE:-$FLEET_LOG_DIR/fleet-up.state}"
# Desired worker count, as a number on one line. `--scale N` writes it; the
# running supervisor reads it every tick and restarts ONLY the dispatcher when
# it changes. A file rather than a signal because the count has to survive the
# supervisor restarting, and because `--scale` is often run when no fleet is up
# at all -- in which case it is simply the count the next start will use.
FLEET_SCALEFILE="${FLEET_SCALEFILE:-$FLEET_LOG_DIR/fleet-up.scale}"
FLEET_WORKTREE_BASE="${FLEET_WORKTREE_BASE:-$OXIDEX_HOME/worktrees/parallel-fix}"
FLEET_DISPATCHER_PGIDS="${FLEET_DISPATCHER_PGIDS:-$FLEET_LOG_DIR/dispatcher-pgids.json}"

# EXIFTOOL_CACHE_DIR is the --cache-dir default for BOTH tiers and is force-set
# into every worker's env by model_fix_loop; pin it here so all three tiers and
# every worker agree.
EXIFTOOL_CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}"
export EXIFTOOL_CACHE_DIR

# Deliberately NOT exported: RUSTC_WRAPPER and CARGO_TARGET_DIR.
# model_fix_loop.py:1305 sets RUSTC_WRAPPER=sccache itself, but only when it is
# not already in the environment -- exporting it here would silently disable
# the config.toml `use_sccache` knob. CARGO_TARGET_DIR is env.pop'd per worker
# on purpose (model_fix_loop.py:857); setting it would collapse 32 worktrees
# onto one target dir and serialise the whole fleet behind one lock.

FLEET_POLL_SECONDS="${FLEET_POLL_SECONDS:-20}"       # supervisor tick
FLEET_MAX_RESTARTS="${FLEET_MAX_RESTARTS:-5}"        # per tier, per window
FLEET_RESTART_WINDOW="${FLEET_RESTART_WINDOW:-1800}" # sec of uptime that forgives past crashes
FLEET_BACKOFF_BASE="${FLEET_BACKOFF_BASE:-10}"       # sec, doubled per consecutive crash
# How often to re-sync worker worktrees onto origin/main while the fleet runs.
# sync_worktrees was called ONCE, immediately before supervise(), so a fleet
# left up for hours produced work against an ever-staler base -- measured
# 2026-07-28: 71 of 72 worker worktrees behind, 11 of them by 21 commits with
# their own uncaptured commits on top. That is exactly how the 155-patch
# backlog became unmergeable. Salvage runs first, so nothing is discarded.
FLEET_RESYNC_SECONDS="${FLEET_RESYNC_SECONDS:-1800}"  # 0 disables
FLEET_BACKOFF_MAX="${FLEET_BACKOFF_MAX:-300}"
FLEET_GRACE_SECONDS="${FLEET_GRACE_SECONDS:-20}"     # SIGTERM -> SIGKILL escalation
FLEET_MIN_FREE_GB="${FLEET_MIN_FREE_GB:-40}"         # disk floor; see preflight_disk
FLEET_MERGER_POLL="${FLEET_MERGER_POLL:-60}"
FLEET_JUDGMENT_POLL="${FLEET_JUDGMENT_POLL:-300}"
FLEET_DISPATCH_TIMEOUT="${FLEET_DISPATCH_TIMEOUT:-2400}"

# Where check_printconv finds ExifTool's Perl. Passing this is not optional in
# practice: with perl_lib=None, validate_fix_commit.check_printconv (line 1216)
# short-circuits to module=None and emits an UNCONDITIONAL
# `printconv-unverifiable` flag, which squad_merge_loop turns into a
# quarantine. 36 of the 80 flags in the ledger on 2026-07-26 were this
# self-inflicted class -- the live mergers had simply been launched without it.
FLEET_PERL_LIB="${FLEET_PERL_LIB:-}"

FLEET_WORKERS="${FLEET_WORKERS:-32}"
FLEET_SQUAD_MODE="${FLEET_SQUAD_MODE:-0}"

# Cap on how many of config.toml's [squads.*] squads get their own merger
# tier. 0 (the default) means "all of them" -- unchanged behaviour for every
# existing caller. A positive N takes the first N squads in config.toml's own
# order; the rest simply get no merger this run, same as if they'd been
# deleted from config.toml, and their branches sit unconsumed until a future
# run raises the cap or covers them directly.
#
# Precedence: --mergers (CLI) > $FLEET_MAX_MERGERS (env) > config.toml's
# [fleet].mergers > 0. Left UNSET here (not defaulted to 0) so cmd_up can
# tell "nothing said anything" apart from an explicit 0 and fall through to
# config.toml -- resolved once $PINNED_CONFIG is known, see config_mergers().
# NOTE: this is the first fleet_up.sh setting config.toml drives at all --
# FLEET_WORKERS has no config.toml source today (env/--workers only), and
# this deliberately does not retrofit one; see the PR description.
FLEET_MAX_MERGERS="${FLEET_MAX_MERGERS:-}"
FLEET_CONFIG="${FLEET_CONFIG:-}"
FLEET_REPO="${OXIDEX_FLEET_REPO:-}"

# How often to repeat the "cannot write the state file" warning while the
# condition persists. The warning goes to a log that is very probably on the
# same filesystem that just filled, so it must not be per-tick.
FLEET_STATE_WARN_EVERY="${FLEET_STATE_WARN_EVERY:-30}"
STATE_WRITE_FAILURES=0

# Resolved by pin_repo(); every tier is invoked as "$PINNED_REPO/scripts/<x>.py".
PINNED_REPO=""
PINNED_CONFIG=""

# Parallel arrays, indexed by tier slot. Bash 4 has associative arrays but
# ordered iteration matters for the status table, so plain indexed arrays with
# a shared index are clearer here.
TIER_TAG=()      # human label, also the log prefix
TIER_KIND=()     # dispatcher | merger | judgment
TIER_ARG=()      # squad name for merger, empty otherwise
TIER_PID=()      # 0 == not running
TIER_PATTERN=()  # substring that MUST appear in the pid's argv (PID-reuse guard)
TIER_RESTARTS=() # consecutive crashes inside the rolling window
TIER_STARTED=()  # epoch of last successful start
TIER_RETRY_AT=() # epoch before which we will not restart (backoff)
TIER_STATE=()    # running | backoff | failed | stopped

SHUTTING_DOWN=0

# ---------------------------------------------------------------------------
# Logging. One consolidated file, one prefix per tier, so that "what happened
# at 02:14" is a single grep instead of a fourteen-file correlation exercise.
# ---------------------------------------------------------------------------

now_epoch() { printf '%(%s)T' -1; }

log() {
    # $1 = tag, rest = message. Written to the consolidated log AND stderr so
    # an interactive operator sees the same stream the log gets.
    local tag=$1
    shift
    local line
    printf -v line '%(%Y-%m-%dT%H:%M:%S)T [%s] %s' -1 "$tag" "$*"
    # Both writes are guarded and the function always returns 0. Logging is
    # the one thing that must still work when the disk is full -- and it is
    # precisely then that an append fails. Unguarded, a failed `>>` under
    # `set -e` would kill the supervisor from inside its own error path (and
    # recurse through the ERR trap, which also logs).
    if [ -n "${FLEET_LOG:-}" ] && [ -d "$(dirname "$FLEET_LOG")" ]; then
        printf '%s\n' "$line" >>"$FLEET_LOG" 2>/dev/null || true
    fi
    printf '%s\n' "$line" >&2 || true
    return 0
}

die() {
    log "fleet-up" "FATAL: $*"
    exit 1
}

log_stream() {
    # Reads a child's merged stdout+stderr and re-emits it into the one
    # consolidated log with a tier prefix. Pure bash on purpose: `sed -u` is
    # GNU-only and BSD sed buffers, which would make the consolidated log lag
    # minutes behind reality exactly when someone is watching it during an
    # incident.
    local tag=$1 line
    while IFS= read -r line; do
        printf '%(%Y-%m-%dT%H:%M:%S)T [%s] %s\n' -1 "$tag" "$line"
    done >>"$FLEET_LOG"
}

# ---------------------------------------------------------------------------
# Process identity. Nothing in this section may ever use `pgrep -f`.
# ---------------------------------------------------------------------------

proc_argv() {
    # Full command line of exactly one pid, or empty if it is gone.
    ps -o command= -p "$1" 2>/dev/null || true
}

pid_matches() {
    # THE liveness primitive: a pid we recorded when we forked it, plus proof
    # that the pid still belongs to the program we started. Both halves are
    # load-bearing:
    #   * the pid alone is not enough -- pids are recycled, and the fleet has
    #     already been bitten by exactly that (2fbf051c "one recycled pgid must
    #     not kill the whole dispatcher").
    #   * the pattern alone is not enough -- that is `pgrep -f`, which matched
    #     its own caller's shell on 2026-07-26 and reported a dead tier alive.
    # Because we look up ONE known pid rather than searching, this function is
    # structurally incapable of matching the checking process.
    local pid=$1 pattern=$2 argv
    [ -n "$pid" ] || return 1
    [ "$pid" -gt 0 ] 2>/dev/null || return 1
    [ "$pid" != "$$" ] || return 1
    argv=$(proc_argv "$pid")
    [ -n "$argv" ] || return 1
    [[ $argv == *"$pattern"* ]]
}

ancestor_pids() {
    # Our own pid plus every parent up to init. These are the pids that make
    # `pgrep -f` lie: the launching shell's argv contains the pattern we are
    # searching for, because it is the command line that started us.
    local pid=${1:-$$} out=""
    while [ -n "$pid" ] && [ "$pid" -gt 1 ] 2>/dev/null; do
        out="$out $pid"
        pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
    done
    printf '%s' "$out "
}

scan_foreign_pids() {
    # Search-by-pattern, used ONLY to detect a fleet somebody else started.
    # $1 = extended regex matched against the full argv.
    # Excludes our own process, our whole ancestor chain, and anything whose
    # argv mentions this script -- the three ways the 2026-07-26 false
    # positive was manufactured.
    local pattern=$1 excl pid argv
    excl=$(ancestor_pids)
    ps -axo pid=,command= | while read -r pid argv; do
        [[ " $excl " == *" $pid "* ]] && continue
        [[ $argv == *fleet_up.sh* ]] && continue
        [[ $argv =~ $pattern ]] && printf '%s\t%s\n' "$pid" "$argv"
    done || true
}

# ---------------------------------------------------------------------------
# Repo pinning -- requirement #2, and the single most important function here.
# ---------------------------------------------------------------------------

repo_head() { git -C "$1" rev-parse HEAD 2>/dev/null || true; }
repo_branch() { git -C "$1" rev-parse --abbrev-ref HEAD 2>/dev/null || true; }

repo_is_main_tracking() {
    # "Main-tracking" means BOTH of:
    #   HEAD == origin/main   -- it is running the code that is actually on main
    #   branch == main        -- because parallel_model_fix_loop.ensure_integration_branch
    #                            (line 405) only retargets round-end merges to
    #                            model-fix-sweep-local when HEAD is LITERALLY
    #                            "main". On any other branch it returns that
    #                            branch as the integration target, and the
    #                            round's merges land there instead. Measured
    #                            2026-07-26: the live dispatcher logged
    #                            "merging into 'feat/fleet-runtime-defect-fixes'"
    #                            for precisely this reason. Pinning the SHA is
    #                            NOT enough; the branch NAME changes behaviour.
    local root=$1 head origin_main branch
    head=$(repo_head "$root")
    branch=$(repo_branch "$root")
    origin_main=$(git -C "$root" rev-parse origin/main 2>/dev/null || true)
    [ -n "$head" ] && [ -n "$origin_main" ] && [ "$head" = "$origin_main" ] && [ "$branch" = "main" ]
}

find_main_worktree() {
    # Ask git, do not guess: list every worktree of this repo and return the
    # first that is main-tracking.
    local from=$1 wt
    while read -r wt; do
        [ -n "$wt" ] || continue
        repo_is_main_tracking "$wt" && { printf '%s' "$wt"; return 0; }
    done < <(git -C "$from" worktree list --porcelain 2>/dev/null | awk '/^worktree /{print $2}')
    return 1
}

primary_checkout() {
    # The repo that owns the shared object store: --git-common-dir is
    # <primary>/.git for every worktree. Used only to find a config.toml, never
    # to run code -- it is the checkout that is habitually on a feature branch.
    local from=$1 common
    common=$(git -C "$from" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)
    [ -n "$common" ] || return 1
    printf '%s' "$(dirname "$common")"
}

pin_repo() {
    # Resolve exactly one absolute checkout and cd into it. Everything after
    # this point uses "$PINNED_REPO/scripts/<x>.py" -- an ABSOLUTE path, which
    # is a complete fix on its own: find_tag_gaps.py:30 derives
    # REPO_ROOT = Path(__file__).resolve().parent.parent from the SCRIPT FILE,
    # and python puts the script's own directory at sys.path[0], so the
    # absolute path pins REPO_ROOT, every `from find_tag_gaps import ...`, and
    # DEFAULT_CONFIG_PATH together. We cd anyway so that any relative path a
    # tier resolves internally lands in the same place.
    local here=$1 candidate

    if [ -n "$FLEET_REPO" ]; then
        candidate=$FLEET_REPO
        [ -d "$candidate/scripts" ] || die "--repo $candidate has no scripts/ directory"
        if ! repo_is_main_tracking "$candidate"; then
            log "fleet-up" "WARNING: --repo $candidate is NOT main-tracking" \
                "(branch=$(repo_branch "$candidate") head=$(repo_head "$candidate")). Round-end merges" \
                "will land on that branch, not model-fix-sweep-local. Proceeding because you asked."
        fi
    elif candidate=$(find_main_worktree "$here"); then
        :
    else
        # Nothing main-tracking exists. Rather than silently running from
        # whatever checkout this script happens to live in -- the exact bug
        # this function exists to prevent -- create the pinned worktree.
        candidate="$OXIDEX_HOME/worktrees/fleet-main"
        if [ "$DRY_RUN" -eq 1 ]; then
            # --dry-run promises to mutate nothing, and creating a worktree
            # plus fast-forwarding local main is a mutation.
            log "fleet-up" "DRY RUN: no main-tracking worktree exists; a real run would create" \
                "$candidate from origin/main. To dry-run the rest now, re-run with" \
                "--repo <an existing checkout>."
            return 10
        fi
        log "fleet-up" "no main-tracking worktree found; creating $candidate"
        git -C "$here" fetch --quiet origin || die "git fetch origin failed"
        if [ -d "$candidate" ]; then
            git -C "$candidate" checkout main --quiet 2>/dev/null \
                || die "$candidate exists but is not a usable main checkout; remove it or pass --repo"
        else
            git -C "$here" worktree add --quiet "$candidate" main \
                || die "could not create a main worktree at $candidate (is 'main' checked out elsewhere? \`git worktree list\`)"
        fi
        # ff-only: this must never rewrite or clobber local main.
        git -C "$candidate" merge --ff-only origin/main --quiet \
            || die "$candidate: local main cannot fast-forward to origin/main -- resolve by hand"
        repo_is_main_tracking "$candidate" \
            || die "$candidate still is not main-tracking after ff (head=$(repo_head "$candidate") branch=$(repo_branch "$candidate"))"
    fi

    PINNED_REPO=$(cd "$candidate" && pwd -P)
    cd "$PINNED_REPO"

    # A pinned checkout that is missing a tier is a doomed run; say which one.
    # The judgment queue is in this list on purpose: a fleet without it is the
    # dead-end configuration that produced "4330 gaps but only 2 landed tags",
    # so starting one has to be a deliberate act (--no-judgment), never a
    # silent consequence of an old checkout.
    local script scripts=(parallel_model_fix_loop.py squad_merge_loop.py)
    [ "$WITH_JUDGMENT" -eq 1 ] && scripts+=(judgment_queue_daemon.py)
    for script in "${scripts[@]}"; do
        [ -f "$PINNED_REPO/scripts/$script" ] || die \
            "pinned checkout $PINNED_REPO has no scripts/$script.
  This usually means the branch adding it has not merged to main yet.
  remedy: merge it, or point the launcher at a checkout that has it:
      $0 --repo /path/to/checkout
  (for judgment_queue_daemon.py only, $0 --no-judgment starts the fleet WITHOUT the
   quarantine tier -- flagged work will accumulate unread, exactly as it did before.)"
    done
    log "fleet-up" "pinned repo: $PINNED_REPO (branch=$(repo_branch "$PINNED_REPO") head=$(repo_head "$PINNED_REPO" | cut -c1-8))"
}

# ---------------------------------------------------------------------------
# Squads
# ---------------------------------------------------------------------------

parse_squads() {
    # Read squad names out of config.toml's [squads.*] tables rather than
    # hardcoding them. The hand-maintained lists drift: the operator brief
    # for this launcher said "7 squads", the squad manifest defines FOURTEEN,
    # and on 2026-07-26 exactly 7 mergers were alive -- the other 7 had been
    # dead since the previous night. exif-core, one of the dead ones, was the
    # 2nd-largest producer in the quarantine ledger (14 of 80 entries). A
    # hardcoded "the 7 that happen to be running" would have cemented that
    # outage forever.
    #
    # Generic over "a TOML file with [squads.*] tables" -- it used to be
    # pointed at the now-deleted scripts/squads.toml and is now pointed at
    # config.toml instead, which carries the identical tables verbatim
    # (moved there so there is exactly one fleet config file; see the PR
    # that did the move). TOML quotes names containing '-'
    # ([squads."sony-minolta"]), so strip them.
    local toml=$1
    [ -f "$toml" ] || return 1
    sed -n 's/^\[squads\.\(.*\)\]$/\1/p' "$toml" | tr -d '"'
}

config_mergers() {
    # [fleet].mergers from config.toml, as raw TOML text -- NOT coerced or
    # validated here, so a garbage value (negative, float, string) flows
    # through to the exact same non-negative-integer check main() already
    # runs on --mergers/$FLEET_MAX_MERGERS, with the same error message,
    # rather than a second, differently-worded validator living here.
    #
    # Prints "0" ("all squads", matching --mergers 0 / FLEET_MAX_MERGERS's
    # pre-existing default) when the key, the [fleet] table, or the file
    # itself is absent or unparseable -- a config.toml written before this
    # key existed silently keeps today's behaviour.
    local toml=$1
    [ -f "$toml" ] || { printf '0'; return 0; }
    python3 -c '
import sys, tomllib
try:
    with open(sys.argv[1], "rb") as f:
        data = tomllib.load(f)
except Exception:
    print(0)
else:
    fleet = data.get("fleet")
    print(fleet.get("mergers", 0) if isinstance(fleet, dict) else 0)
' "$toml" 2>/dev/null || printf '0'
}

# ---------------------------------------------------------------------------
# Preflight -- requirement #1. Every check fails LOUDLY with the remedy.
# ---------------------------------------------------------------------------

PREFLIGHT_ERRORS=()
preflight_fail() { PREFLIGHT_ERRORS+=("$1"); }

preflight_gh() {
    command -v gh >/dev/null 2>&1 || {
        preflight_fail "gh not on PATH -- auto_publish_round cannot open or merge the sweep PR.
  remedy: brew install gh && gh auth login"
        return
    }
    gh auth status >/dev/null 2>&1 || {
        preflight_fail "gh is not authenticated -- the sweep PR would be created and then never merge.
  remedy: gh auth login   (needs 'repo' and 'workflow' scopes)"
        return
    }
    local url
    url=$(git -C "$PINNED_REPO" remote get-url origin 2>/dev/null || true)
    case "$url" in
        https://*) : ;;
        "") preflight_fail "pinned repo has no 'origin' remote -- nothing can be published." ;;
        *)  # Measured on this machine: two gh accounts are configured and the
            # SSH key belongs to the one WITHOUT push access, so an ssh remote
            # turns every publish into a silent failure at the last step.
            preflight_fail "origin is '$url' (not https). gh's token auth only covers https pushes here;
  an ssh remote pushes as whatever key the agent holds, which on this machine is a
  different GitHub account with no write access -- every sweep would fail at push.
  remedy: git -C $PINNED_REPO remote set-url origin https://github.com/<owner>/<repo>.git" ;;
    esac
}

resolve_config() {
    # config.toml is gitignored, so `git worktree add` never copies it and the
    # pinned checkout almost certainly does NOT have one. Measured 2026-07-26:
    # the main-tracking worktree had no config.toml at all, and the dispatcher
    # exits 1 immediately (parallel_model_fix_loop.py:2763) when --config is
    # missing. Search a defined order and report every path we looked at.
    local candidates=() primary c
    [ -n "$FLEET_CONFIG" ] && candidates+=("$FLEET_CONFIG")
    candidates+=("$PINNED_REPO/config.toml")
    if primary=$(primary_checkout "$PINNED_REPO"); then
        candidates+=("$primary/config.toml")
    fi
    candidates+=("$OXIDEX_HOME/config.toml")
    for c in "${candidates[@]}"; do
        if [ -f "$c" ]; then
            PINNED_CONFIG=$c
            return 0
        fi
    done
    preflight_fail "no config.toml found. Searched:
$(printf '    %s\n' "${candidates[@]}")
  remedy: cp $PINNED_REPO/config.example.toml <one of the above> and fill in the API key,
          or pass --config /abs/path/config.toml"
    return 1
}

preflight_config() {
    resolve_config || return
    # "Present" is not "usable": a truncated or half-edited config fails deep
    # inside the first worker, minutes later, as a stack trace nobody reads.
    if ! python3 -c 'import sys,tomllib;tomllib.load(open(sys.argv[1],"rb"))' \
        "$PINNED_CONFIG" 2>/dev/null; then
        preflight_fail "config.toml is not parseable TOML: $PINNED_CONFIG
  remedy: fix the syntax; reproduce the exact error with
      python3 -c 'import sys,tomllib;tomllib.load(open(sys.argv[1],\"rb\"))' $PINNED_CONFIG"
    fi
}

preflight_sccache() {
    # Not a nicety. 32 workers + 14 mergers all build the same crate; without a
    # shared compilation cache the fleet is disk- and CPU-bound on rustc from
    # the first round. Measured hit rate on this machine: 6145 hits / 1738
    # misses.
    if [ "${OXIDEX_USE_SCCACHE:-1}" = "0" ]; then
        return
    fi
    command -v sccache >/dev/null 2>&1 || preflight_fail \
        "sccache not on PATH. 32 workers x 14 mergers rebuilding the same crate without a
  shared cache will saturate the disk before it saturates the CPU.
  remedy: brew install sccache   (or set OXIDEX_USE_SCCACHE=0 to accept the cost)"
}

preflight_uv() {
    command -v uv >/dev/null 2>&1 || preflight_fail \
        "uv not on PATH -- the dispatcher spawns every worker as \`uv run scripts/model_fix_loop.py\`.
  remedy: brew install uv"
}

preflight_exiftool_cache() {
    local cache=$EXIFTOOL_CACHE_DIR
    [ -d "$cache/exiftool" ] || {
        preflight_fail "exiftool source cache missing at $cache/exiftool -- every PrintConv check would
  abstain and every commit would quarantine as printconv-unverifiable.
  remedy: run \`just compare-exiftool\` once, or set EXIFTOOL_CACHE_DIR to a populated cache"
        return
    }
    # A samples dir that exists but is empty is worse than one that is absent:
    # find_tag_gaps reports zero gaps and the fleet declares victory.
    if [ ! -d "$cache/combined-samples" ] || [ -z "$(ls -A "$cache/combined-samples" 2>/dev/null)" ]; then
        preflight_fail "sample corpus $cache/combined-samples is missing or EMPTY. Every format would
  report zero gaps and the fleet would idle while looking healthy.
  remedy: repopulate it (just compare-exiftool-samples) before starting"
    fi
}

preflight_perl_lib() {
    if [ -z "$FLEET_PERL_LIB" ]; then
        # Same resolution the rest of the fleet uses, but resolved HERE so the
        # failure is one line in preflight instead of 14 mergers each silently
        # flagging every commit unverifiable.
        #
        # The PINNED tree, not a Cellar glob. The glob took whatever brew last
        # installed -- 13.55 on 2026-08-01 -- while src/exiftool_tables is
        # transcribed from the 13.59 checkout in $EXIFTOOL_CACHE_DIR. Different
        # releases spell different PrintConv maps, so a correct transcription
        # reads as a fabricated pair and a fabricated one can read as correct.
        # There is no "close enough" version here, which is why the fallback
        # is gone rather than demoted.
        FLEET_PERL_LIB=$EXIFTOOL_CACHE_DIR/exiftool/lib
    fi
    if [ -z "$FLEET_PERL_LIB" ] || [ ! -d "$FLEET_PERL_LIB" ]; then
        preflight_fail "no ExifTool Perl lib at $FLEET_PERL_LIB (the pinned tree
  \$EXIFTOOL_CACHE_DIR/exiftool/lib, overridable with --perl-lib or FLEET_PERL_LIB).
  Without it validate_fix_commit.check_printconv abstains and EVERY commit that adds a
  lookup value is quarantined as printconv-unverifiable -- 36 of the 80 flags in the
  ledger on 2026-07-26 were exactly this, caused by mergers launched without --perl-lib.
  remedy: populate the pinned cache (\`just compare-exiftool\` once), or point
  FLEET_PERL_LIB at a checkout of the SAME release the tables were transcribed from.
  A brew Cellar lib is NOT interchangeable unless its version matches."
    fi
}

free_gb() {
    # POSIX df, 1K blocks, "Available" column. -P keeps long device names on
    # one line, which BSD df otherwise wraps.
    df -Pk "$1" 2>/dev/null | awk 'NR==2 {printf "%d", $4/1048576}'
}

preflight_disk() {
    # Disk, not CPU, is this fleet's real bottleneck: 32 worker worktrees each
    # with their own cargo target dir, plus a sample corpus, plus per-request
    # JSON logs. Measured 2026-07-26: / was 93% full (71G of 927G free) while
    # ~/.oxidex/logs/dashboard.log.1 alone was 186MB and
    # ~/.oxidex/logs/model-fix-requests/ held 41,551 files -- and the janitor
    # that prunes both only runs under --squad-mode or --enable-janitor,
    # neither of which the legacy dispatcher turns on.
    local avail
    avail=$(free_gb "$OXIDEX_HOME")
    [ -n "$avail" ] || { preflight_fail "could not read free disk for $OXIDEX_HOME"; return; }
    if [ "$avail" -lt "$FLEET_MIN_FREE_GB" ]; then
        preflight_fail "only ${avail}G free on the volume holding $OXIDEX_HOME (floor is ${FLEET_MIN_FREE_GB}G).
  32 worker target dirs will fill that and every tier will start failing builds mid-round.
  remedy, in order of yield:
    rm -f $FLEET_LOG_DIR/dashboard.log.*            # rotated dashboard logs, often >100MB
    find $FLEET_LOG_DIR/model-fix-requests -type f -mtime +2 -delete
    cargo clean in idle worktrees, or raise the floor with FLEET_MIN_FREE_GB=<n>"
    else
        log "fleet-up" "disk: ${avail}G free (floor ${FLEET_MIN_FREE_GB}G)"
    fi
}

preflight_no_existing_fleet() {
    # Starting a second dispatcher on top of a live one is not merely
    # redundant: two mergers for the same squad launched from DIFFERENT
    # checkouts SIGTERM each other on sight (distill_lessons.acquire_lock:778
    # kills any holder whose script_git_sha differs), so they crash-loop
    # forever while looking busy.
    local found=""

    # Tier 1 has an authoritative probe that no pattern match can beat: the
    # dispatcher holds an fcntl flock on logs/dispatcher.lock for its entire
    # lifetime, and the kernel drops it however abruptly the process dies. No
    # staleness window, no self-match.
    local lock="$FLEET_LOG_DIR/dispatcher.lock"
    if [ -f "$lock" ]; then
        local holder
        holder=$(python3 - "$lock" <<'PY' || true
import fcntl, sys
path = sys.argv[1]
try:
    fh = open(path, "a+")
    fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    fh.seek(0)
    print(fh.read().strip() or "?")
except OSError:
    pass
PY
)
        [ -n "$holder" ] && found="${found}
    dispatcher: holding $lock (pid $holder)"
    fi

    local hits
    hits=$(scan_foreign_pids 'squad_merge_loop\.py|judgment_queue_daemon\.py|parallel_model_fix_loop\.py' || true)
    # sed, not printf: `hits` is multi-line and printf would emit the whole
    # blob against one format specifier, indenting only the first process.
    # The interpreter is shortened to its basename because the real one is a
    # 130-character Homebrew framework path that pushes the part an operator
    # actually needs -- which script, which squad -- off the screen.
    [ -n "$hits" ] && found="${found}
$(printf '%s\n' "$hits" | sed -E 's#(^[0-9]+	)[^ ]*/([^/ ]+)#\1\2#; s/^/    /')"

    if [ -f "$FLEET_PIDFILE" ]; then
        local prev
        prev=$(cat "$FLEET_PIDFILE" 2>/dev/null || true)
        pid_matches "$prev" "fleet_up.sh" && found="${found}
    another fleet_up.sh supervisor: pid $prev"
    fi

    if [ -n "$found" ]; then
        preflight_fail "a fleet is ALREADY running:$found
  Starting a second one makes mergers for the same squad SIGTERM each other in a loop.
  remedy: $0 --down            (if this launcher started it)
          uv run $PINNED_REPO/scripts/stop_parallel_fix.py   (if it was hand-launched)"
    fi
}

run_preflight() {
    PREFLIGHT_ERRORS=()
    preflight_gh
    preflight_config
    preflight_sccache
    preflight_uv
    preflight_exiftool_cache
    preflight_perl_lib
    preflight_disk
    preflight_no_existing_fleet
    if [ ${#PREFLIGHT_ERRORS[@]} -gt 0 ]; then
        log "fleet-up" "PREFLIGHT FAILED (${#PREFLIGHT_ERRORS[@]} problem(s)); nothing was started."
        local e
        for e in "${PREFLIGHT_ERRORS[@]}"; do
            printf '\n  * %s\n' "$e" >&2
        done
        printf '\n' >&2
        return 1
    fi
    log "fleet-up" "preflight OK"
}

# ---------------------------------------------------------------------------
# Worktree salvage + sync -- requirement #3.
# ---------------------------------------------------------------------------

worktree_unique_commits() {
    # Commits reachable from HEAD but NOT from origin/main -- i.e. everything
    # strictly beyond the merge-base, which is the only correct definition of
    # "unpublished work in this worktree".
    #
    # DO NOT replace this with `git diff <branch> main`. A diff measures TOTAL
    # DIVERGENCE, so a worktree that is merely BEHIND main (the overwhelmingly
    # common case -- 30 of 32 worker worktrees sat exactly there on 2026-07-26)
    # produces a large diff and gets "salvaged" onto a junk branch. That noise
    # is what makes an operator start ignoring salvage branches, which is how a
    # real one gets deleted.
    local wt=$1
    git -C "$wt" rev-list --count origin/main..HEAD 2>/dev/null || printf '0'
}

worktree_dirty() {
    # Tracked modifications only. Untracked files in a worker worktree are
    # normally build spill; a `git reset --hard` does not touch them anyway.
    local wt=$1
    git -C "$wt" status --porcelain --untracked-files=no 2>/dev/null || true
}

salvage_worktree() {
    # Preserve everything unpublished on salvage/<name>-<stamp> and return the
    # branch name, or nothing when there was nothing to preserve.
    local wt=$1 name=$2 stamp=$3 tip wip branch
    tip=$(git -C "$wt" rev-parse HEAD 2>/dev/null || true)
    [ -n "$tip" ] || return 1
    local unique dirty
    unique=$(worktree_unique_commits "$wt")
    # Never let an empty or non-numeric answer reach `-eq`: under `set -e`
    # that is a hard abort of the whole launcher, and it would abort DURING
    # the salvage pass -- the one moment when stopping half-done is worst.
    # An unreadable worktree must fall back to "assume it has work", never to
    # "assume it is disposable".
    case "$unique" in ''|*[!0-9]*) unique=1 ;; esac
    dirty=$(worktree_dirty "$wt")
    if [ "$unique" -eq 0 ] && [ -z "$dirty" ]; then
        return 1
    fi
    if [ -n "$dirty" ]; then
        # `stash create` writes a commit OBJECT and returns its sha WITHOUT
        # touching the index or the working tree. That ordering matters: if we
        # die between salvaging and resetting, nothing has been disturbed, and
        # the salvage branch already holds the uncommitted work. `git stash
        # push` would mutate the worktree here and leave a stash entry that the
        # next `checkout -B` silently strands.
        wip=$(git -C "$wt" stash create "fleet_up salvage $name" 2>/dev/null || true)
        [ -n "$wip" ] && tip=$wip
    fi
    branch="salvage/$name-$stamp"
    git -C "$wt" branch -f "$branch" "$tip" >/dev/null 2>&1 || return 1
    printf '%s' "$branch"
}

sync_worktrees() {
    # Salvage FIRST, then reset. Never the other way round.
    #
    # Periodic resync passes `preserve-work`: a dirty worktree or one with an
    # unpublished commit belongs to an in-flight worker and must not be reset
    # underneath that process. Startup keeps the default `reset-all` behavior,
    # because no worker exists yet and leftovers from a previous crash need to
    # be cleaned after they are salvaged.
    local base=${1:-$FLEET_WORKTREE_BASE} mode=${2:-reset-all}
    local stamp wt name branch synced=0 saved=0 preserved=0 had_work
    stamp=$(date +%Y%m%d-%H%M%S)
    [ -d "$base" ] || { log "fleet-up" "no worker worktrees under $base -- nothing to sync"; return 0; }
    for wt in "$base"/*; do
        [ -e "$wt/.git" ] || continue
        name=$(basename "$wt")
        if ! git -C "$wt" rev-parse --verify HEAD >/dev/null 2>&1; then
            log "sync" "SKIP $name: no resolvable HEAD (mid-rebase or freshly created?)"
            continue
        fi
        had_work=0
        if branch=$(salvage_worktree "$wt" "$name" "$stamp"); then
            log "sync" "salvaged $name -> $branch"
            saved=$((saved + 1))
            had_work=1
        fi
        if [ "$mode" = preserve-work ] && [ "$had_work" -eq 1 ]; then
            log "sync" "PRESERVE $name: in-flight work left untouched during periodic resync"
            preserved=$((preserved + 1))
            continue
        fi
        # Moves the worktree's current branch to origin/main. Deliberately no
        # `git clean`: -fd would delete an untracked half-written parser, and
        # -x would delete the target/ dir this fleet's throughput depends on.
        if git -C "$wt" reset --hard origin/main --quiet 2>/dev/null; then
            synced=$((synced + 1))
        else
            log "sync" "WARNING $name: reset to origin/main failed; leaving it alone"
        fi
    done
    log "fleet-up" "worktree sync: $synced reset to origin/main, $saved salvaged, $preserved preserved"
}

# ---------------------------------------------------------------------------
# Tier lifecycle
# ---------------------------------------------------------------------------

SPAWNED_PID=0

spawn() {
    # Start a child whose stdout+stderr flow into the consolidated log with a
    # tier prefix, and publish its REAL pid in SPAWNED_PID.
    #
    # Two non-obvious things here, both learned by breaking them:
    #
    # 1. `( exec ... )` -- `exec` REPLACES the subshell image, so the pid bash
    #    reports in $! IS the python process, not a wrapper that will exit and
    #    orphan it. A plain `cmd | prefixer &` yields the PREFIXER's pid, and
    #    every later liveness check would then be measuring the wrong process.
    #
    # 2. The result comes back in a GLOBAL, not on stdout. `pid=$(spawn ...)`
    #    deadlocks: the `>(log_stream)` process substitution inherits the
    #    command substitution's stdout pipe, so `$( )` never sees EOF and the
    #    launcher hangs forever after starting exactly one tier. The
    #    `>/dev/null` on log_stream closes that inherited fd for the same
    #    reason -- log_stream writes to the log file explicitly and must not
    #    keep any caller's pipe alive.
    local tag=$1
    shift
    (
        exec > >(log_stream "$tag" >/dev/null 2>&1) 2>&1
        exec "$@"
    ) &
    SPAWNED_PID=$!
}

tier_add() {
    # tag kind arg pattern
    local i=${#TIER_TAG[@]}
    TIER_TAG[i]=$1
    TIER_KIND[i]=$2
    TIER_ARG[i]=$3
    TIER_PATTERN[i]=$4
    TIER_PID[i]=0
    TIER_RESTARTS[i]=0
    TIER_STARTED[i]=0
    TIER_RETRY_AT[i]=0
    TIER_STATE[i]=stopped
}

tier_start() {
    local i=$1
    SPAWNED_PID=0
    case "${TIER_KIND[i]}" in
        dispatcher)
            # --auto-publish is BooleanOptionalAction with default None, and
            # parallel_model_fix_loop.py:2798 resolves it to args.infinite --
            # so --infinite already turns auto-publish ON. Passing the flag
            # explicitly anyway, because "auto-publish is on" is the entire
            # point of this launcher and it must not depend on a default that
            # a future refactor could flip.
            local dispatcher_mode=()
            if [ "$FLEET_SQUAD_MODE" -eq 1 ]; then
                dispatcher_mode+=(--squad-mode)
            fi
            spawn "${TIER_TAG[i]}" \
                python3 -u "$PINNED_REPO/scripts/parallel_model_fix_loop.py" \
                --config "$PINNED_CONFIG" \
                --max-parallel "$FLEET_WORKERS" \
                --timeout "$FLEET_DISPATCH_TIMEOUT" \
                --cache-dir "$EXIFTOOL_CACHE_DIR" \
                --home "$OXIDEX_HOME" \
                "${dispatcher_mode[@]}" \
                --infinite --round-delay 0 --auto-publish
            ;;
        merger)
            # squad_merge_loop now DOES take --config (it reads this squad's
            # ownership/formats out of config.toml's [squads.*] tables) --
            # pass the already-resolved $PINNED_CONFIG explicitly rather than
            # relying on squad_merge_loop.py's own default. Its default only
            # checks "next to the checkout" (repo_root/config.toml), but
            # config.toml is gitignored/per-installation and resolve_config
            # already searched the REAL set of candidate locations (it can
            # just as easily be $OXIDEX_HOME/config.toml) -- relying on the
            # narrower built-in default here would silently degrade a merger
            # to "no squads found" on exactly the installations where
            # config.toml is NOT next to the checkout.
            spawn "${TIER_TAG[i]}" \
                python3 -u "$PINNED_REPO/scripts/squad_merge_loop.py" \
                --squad "${TIER_ARG[i]}" \
                --repo "$PINNED_REPO" \
                --config "$PINNED_CONFIG" \
                --home "$OXIDEX_HOME" \
                --cache-dir "$EXIFTOOL_CACHE_DIR" \
                --perl-lib "$FLEET_PERL_LIB" \
                --infinite --poll-seconds "$FLEET_MERGER_POLL"
            ;;
        judgment)
            # --apply is REQUIRED: the daemon's default is a complete but
            # read-only dry run, on purpose. Without it the quarantine tier
            # runs, reports, and changes nothing -- which is indistinguishable
            # from the dead end it was written to replace.
            #
            # --config: same reasoning as the merger case above -- the
            # daemon's re-admission path validates against squad ownership
            # globs read from config.toml, and its own built-in default must
            # not be trusted to find a gitignored, per-installation file.
            spawn "${TIER_TAG[i]}" \
                python3 -u "$PINNED_REPO/scripts/judgment_queue_daemon.py" \
                --repo "$PINNED_REPO" \
                --config "$PINNED_CONFIG" \
                --home "$OXIDEX_HOME" \
                --cache-dir "$EXIFTOOL_CACHE_DIR" \
                --perl-lib "$FLEET_PERL_LIB" \
                --infinite --poll-seconds "$FLEET_JUDGMENT_POLL" --apply
            ;;
        *) die "unknown tier kind ${TIER_KIND[i]}" ;;
    esac
    TIER_PID[i]=$SPAWNED_PID
    TIER_STARTED[i]=$(now_epoch)
    TIER_STATE[i]=running
    log "fleet-up" "started ${TIER_TAG[i]} (pid $SPAWNED_PID)"
}

tier_alive() {
    local i=$1
    pid_matches "${TIER_PID[i]}" "${TIER_PATTERN[i]}"
}

owned_formats() {
    # Formats the named squad EXCLUSIVELY consumes, per #209's
    # format_owner_map. Asked of fleet_health.py rather than reimplemented in
    # bash on purpose: the ownership rules (module-name match, then most
    # specialised claimant, then name order) live in exactly one place, and a
    # second copy here would drift the first time config.toml's squad
    # manifest changes.
    #
    # Best-effort and always rc 0 -- a supervisor must not die because it
    # could not enrich a log line.
    local squad=$1 out
    [ -n "$squad" ] || return 0
    [ -n "${PINNED_REPO:-}" ] || return 0
    [ -n "${PINNED_CONFIG:-}" ] || return 0
    out=$(python3 "$PINNED_REPO/scripts/fleet_health.py" \
            --config "$PINNED_CONFIG" \
            --formats-for "$squad" 2>/dev/null | tr '\n' ' ') || out=""
    printf '%s' "${out% }"
    return 0
}

owned_formats_note() {
    # " [owns: JPEG]" or "" -- suffix for the DEAD line, so the blast radius
    # is visible on the first line about the death rather than only after a
    # tier exhausts its restart budget.
    local fmts
    fmts=$(owned_formats "${TIER_ARG[$1]}")
    [ -n "$fmts" ] && printf ' [exclusively owns: %s]' "$fmts"
    return 0
}

backoff_seconds() {
    # Exponential with a ceiling: a tier that dies because the disk filled
    # should not hammer the disk, but a tier that died once on a transient
    # network error should come back in seconds.
    local n=$1 base=${2:-$FLEET_BACKOFF_BASE} max=${3:-$FLEET_BACKOFF_MAX} d=$1
    d=$base
    while [ "$n" -gt 1 ]; do
        d=$((d * 2))
        n=$((n - 1))
        [ "$d" -ge "$max" ] && { d=$max; break; }
    done
    [ "$d" -gt "$max" ] && d=$max
    printf '%s' "$d"
}

write_state() {
    # The pidfile and state file are what make --status and --down EXACT.
    # Written atomically so a --status racing a restart never reads a half
    # file.
    #
    # NOTHING IN HERE MAY KILL THE SUPERVISOR. This function is bookkeeping;
    # restarting dead tiers is the mission, and losing the former must never
    # cost the latter.
    #
    # It ran under `set -e` with an unguarded `mv` until 2026-07-30, when the
    # disk filled at 23:40:41. Eleven of fourteen mergers died on ENOSPC.
    # supervise() did its job -- it detected five of them and logged
    # `restart 1/5 in 10s` for each. Then this function's own `mv` returned
    # ENOSPC and `set -e` killed the supervisor BETWEEN scheduling those
    # restarts and performing them. The last two lines of its stderr were:
    #
    #     2026-07-30T23:40:43 [fleet-up] merger:standards-appn restart 1/5 in 10s
    #     mv: No space left on device (os error 28)
    #
    # No "fleet down", no FATAL, no trap -- `set -e` exits silently. It left
    # $FLEET_STATEFILE.96880 on disk holding the CORRECT new state (five
    # tiers moved to `0 backoff`) that the rename never installed, so the
    # live state file went on claiming all fourteen mergers were `running`
    # for the next hour while nothing supervised them. A supervisor that
    # cannot record what it is doing must keep doing it.
    local tmp=$FLEET_STATEFILE.$$
    local ok=1
    {
        printf 'supervisor\t%s\t%s\tfleet_up.sh\n' "$$" running
        local i
        for i in "${!TIER_TAG[@]}"; do
            printf '%s\t%s\t%s\t%s\n' \
                "${TIER_TAG[i]}" "${TIER_PID[i]}" "${TIER_STATE[i]}" "${TIER_PATTERN[i]}"
        done
    } >"$tmp" 2>/dev/null || ok=0
    if [ "$ok" -eq 1 ]; then
        mv -f "$tmp" "$FLEET_STATEFILE" 2>/dev/null || ok=0
    fi
    if [ "$ok" -eq 1 ]; then
        printf '%s\n' "$$" >"$FLEET_PIDFILE" 2>/dev/null || ok=0
    fi

    if [ "$ok" -eq 1 ]; then
        # Recovered: say so once, so the gap in the state file's mtime is
        # explainable after the fact rather than mysterious.
        if [ "$STATE_WRITE_FAILURES" -gt 0 ]; then
            log "fleet-up" "state file writable again after $STATE_WRITE_FAILURES failed attempt(s)"
            STATE_WRITE_FAILURES=0
        fi
        return 0
    fi

    # Failed. Do not leave the partial temp behind (it is what filled the
    # disk's last bytes in the first place), and do not spam a log that is
    # very probably on the same full filesystem -- warn on the 1st failure
    # and then every FLEET_STATE_WARN_EVERY-th.
    rm -f "$tmp" 2>/dev/null || true
    STATE_WRITE_FAILURES=$((STATE_WRITE_FAILURES + 1))
    if [ "$STATE_WRITE_FAILURES" -eq 1 ] \
       || [ $((STATE_WRITE_FAILURES % FLEET_STATE_WARN_EVERY)) -eq 0 ]; then
        log "fleet-up" "WARNING cannot write $FLEET_STATEFILE" \
            "(attempt $STATE_WRITE_FAILURES; disk full?). --status/--down are STALE" \
            "until this clears; supervision continues. $(disk_free_human)"
    fi
    return 0
}

disk_free_human() {
    # Best-effort one-liner for the state-write warning. Never fails: a
    # supervisor must not die trying to explain why it is unhappy.
    local free
    free=$(df -h "$FLEET_LOG_DIR" 2>/dev/null | awk 'NR==2 {print $4}' 2>/dev/null) || free=""
    [ -n "$free" ] && printf 'free=%s' "$free" || printf 'free=unknown'
    return 0
}

resync_due() {
    # Should supervise() re-sync worker worktrees now? Split out from the loop
    # because supervise() cannot be exercised without the entire tier array
    # set plus its fd-3 sleep channel, and the POLICY is the part worth
    # pinning: fire once the interval has elapsed, never when disabled.
    local now=$1 last=$2 interval=$3
    [ "$interval" -gt 0 ] || return 1
    [ $((now - last)) -ge "$interval" ]
}

dispatcher_workers_active() {
    # parallel_model_fix_loop atomically persists every live worker process
    # group here. A worktree can still be completely clean while its model is
    # reading the old base or composing a patch; resetting that checkout is
    # therefore unsafe even though preserve-work has nothing to salvage yet.
    #
    # Do not trust the file alone after a dispatcher crash: validate each
    # recorded process group with killpg(0). A missing, torn, empty, or wholly
    # stale file correctly means that no live worker blocks the resync.
    local path=${1:-$FLEET_DISPATCHER_PGIDS}
    [ -r "$path" ] || return 1
    python3 - "$path" <<'PY'
import json
import os
import sys

try:
    with open(sys.argv[1], encoding="utf-8") as f:
        pgids = json.load(f).get("pgids", [])
except (OSError, ValueError, AttributeError):
    # Fail closed. The dispatcher writes this file atomically, so malformed
    # live state is exceptional; resetting worktrees on the optimistic guess
    # that it means "idle" would recreate the destructive race this guard is
    # here to prevent. A live dispatcher repairs the file on its next worker
    # register/unregister, while a restarted one clears it during orphan reap.
    raise SystemExit(0)

if not isinstance(pgids, list):
    raise SystemExit(0)

for raw in pgids:
    try:
        pgid = int(raw)
    except (TypeError, ValueError, OverflowError):
        continue
    if pgid <= 1:
        continue
    try:
        os.killpg(pgid, 0)
    except (ProcessLookupError, OverflowError):
        continue
    except PermissionError:
        # It exists even if this account cannot signal it.
        raise SystemExit(0)
    else:
        raise SystemExit(0)
raise SystemExit(1)
PY
}

read_scale_request() {
    # The desired worker count, or empty when there is no valid request.
    # Deliberately silent on garbage: a corrupt scale file must never take the
    # supervisor down, it must just leave the fleet at its current size.
    local want
    [ -f "$FLEET_SCALEFILE" ] || return 0
    # First line, trimmed at the EDGES only. Stripping all whitespace instead
    # would silently reinterpret a malformed "3 4" as a request for 34 workers
    # -- a garbage file must be ignored, never quietly turned into a plausible
    # number the operator never asked for.
    want=$(head -n 1 "$FLEET_SCALEFILE" 2>/dev/null || true)
    want=${want#"${want%%[![:space:]]*}"}
    want=${want%"${want##*[![:space:]]}"}
    case "$want" in
        ''|*[!0-9]*) return 0 ;;
    esac
    [ "$want" -gt 0 ] 2>/dev/null || return 0
    printf '%s' "$want"
}

apply_scale_request() {
    # Restart ONLY the dispatcher when the requested worker count changes.
    # The mergers and the judgment queue are untouched: they do not carry the
    # worker count, and bouncing them would throw away a merger's in-progress
    # batch check for no reason.
    local want i pid waited
    want=$(read_scale_request)
    [ -n "$want" ] || return 0
    [ "$want" != "$FLEET_WORKERS" ] || return 0
    log "fleet-up" "scale: workers $FLEET_WORKERS -> $want (restarting dispatcher only)"
    FLEET_WORKERS=$want
    for i in "${!TIER_TAG[@]}"; do
        [ "${TIER_KIND[i]}" = dispatcher ] || continue
        pid=${TIER_PID[i]}
        if [ "$pid" -gt 0 ] 2>/dev/null; then
            # Drain rather than race: the dispatcher's own SIGTERM handler is
            # what kills its workers. Starting the replacement while the old
            # one still owns worker process groups gives two dispatchers the
            # same worktrees, which is the "mergers SIGTERM each other" failure
            # one tier down.
            kill -TERM "$pid" 2>/dev/null || true
            waited=0
            while [ "$waited" -lt "$FLEET_GRACE_SECONDS" ] && kill -0 "$pid" 2>/dev/null; do
                sleep 1
                waited=$((waited + 1))
            done
            kill -0 "$pid" 2>/dev/null && { log "fleet-up" "SIGKILL dispatcher (pid $pid) after ${waited}s"; kill -KILL "$pid" 2>/dev/null || true; }
        fi
        # A deliberate scale is NOT a crash: leave TIER_RESTARTS alone so a
        # fleet that is resized a few times does not exhaust FLEET_MAX_RESTARTS
        # and give up on a tier that never actually failed.
        TIER_PID[i]=0
        TIER_STATE[i]=stopped
        tier_start "$i"
    done
    write_state
}

supervise() {
    local i now uptime delay last_resync resync_deferred=0
    last_resync=$(now_epoch)
    while [ "$SHUTTING_DOWN" -eq 0 ]; do
        now=$(now_epoch)
        # Before liveness: a scaled dispatcher is stopped and restarted here,
        # so the loop below must not also see it "dead" and bill it a restart.
        apply_scale_request

        # Keep the bases fresh. Work written against a stale base conflicts
        # with everything that landed since, and the fleet has no other path
        # that refreshes worker worktrees mid-run.
        if resync_due "$now" "$last_resync" "$FLEET_RESYNC_SECONDS"; then
            if dispatcher_workers_active; then
                # Dirty/committed work is not the only in-flight state. A
                # worker can be waiting on the provider with a clean checkout
                # after its prompt captured the old files; moving HEAD then
                # makes the returned patch target a different base. Leave the
                # deadline due and retry on later ticks until the dispatcher
                # reaches its comparison/between-round window.
                if [ "$resync_deferred" -eq 0 ]; then
                    log "fleet-up" "periodic worktree sync deferred: live worker process groups"
                    resync_deferred=1
                fi
            else
                sync_worktrees "$FLEET_WORKTREE_BASE" preserve-work
                last_resync=$now
                resync_deferred=0
            fi
        fi
        local live=0 failed=0
        for i in "${!TIER_TAG[@]}"; do
            [ "${TIER_STATE[i]}" = failed ] && { failed=$((failed + 1)); continue; }
            if tier_alive "$i"; then
                live=$((live + 1))
                uptime=$((now - TIER_STARTED[i]))
                # Rolling window: a tier that has stayed up long enough has
                # earned its restart budget back. Without this, a fleet left
                # running for a week eventually exhausts the cap on unrelated,
                # widely-spaced crashes and shuts itself down for no reason.
                if [ "${TIER_RESTARTS[i]}" -gt 0 ] && [ "$uptime" -ge "$FLEET_RESTART_WINDOW" ]; then
                    log "fleet-up" "${TIER_TAG[i]} healthy for ${uptime}s -- restart budget reset"
                    TIER_RESTARTS[i]=0
                fi
                continue
            fi
            # Dead.
            if [ "${TIER_STATE[i]}" = running ]; then
                log "fleet-up" "${TIER_TAG[i]} (pid ${TIER_PID[i]}) is DEAD$(owned_formats_note "$i")"
                TIER_PID[i]=0
                TIER_RESTARTS[i]=$((TIER_RESTARTS[i] + 1))
                if [ "${TIER_RESTARTS[i]}" -gt "$FLEET_MAX_RESTARTS" ]; then
                    # Surface, do not spin. A tier that cannot stay up is a bug
                    # report, and burying it under an infinite restart loop is
                    # how the fleet historically looked healthy while doing
                    # nothing.
                    TIER_STATE[i]=failed
                    log "fleet-up" "GIVING UP on ${TIER_TAG[i]}: ${TIER_RESTARTS[i]} restarts" \
                        "exceeded FLEET_MAX_RESTARTS=$FLEET_MAX_RESTARTS. Investigate its lines in $FLEET_LOG."
                    # Since #209 each format has exactly ONE consuming squad,
                    # so a permanently failed merger is a TOTAL LOSS for the
                    # formats it owns -- not degraded throughput. Name them
                    # here: "merger:standards-appn failed" means nothing to a
                    # tired operator, "JPEG now has no consumer" does.
                    local lost
                    lost=$(owned_formats "${TIER_ARG[i]}")
                    if [ -n "$lost" ]; then
                        log "fleet-up" "ALARM ${TIER_TAG[i]} is the EXCLUSIVE owner of: $lost." \
                            "Work for those formats is now stranded -- no other squad consumes them." \
                            "Check: $PINNED_REPO/scripts/fleet_health.py"
                    fi
                    failed=$((failed + 1))
                    continue
                fi
                delay=$(backoff_seconds "${TIER_RESTARTS[i]}")
                TIER_RETRY_AT[i]=$((now + delay))
                TIER_STATE[i]=backoff
                log "fleet-up" "${TIER_TAG[i]} restart ${TIER_RESTARTS[i]}/$FLEET_MAX_RESTARTS in ${delay}s"
            fi
            if [ "${TIER_STATE[i]}" = backoff ] && [ "$now" -ge "${TIER_RETRY_AT[i]}" ]; then
                tier_start "$i"
                live=$((live + 1))
            fi
        done
        write_state
        if [ "$failed" -eq "${#TIER_TAG[@]}" ]; then
            log "fleet-up" "every tier has failed permanently -- exiting so this surfaces"
            return 1
        fi
        # `read -t` on fd 3 rather than `sleep`: bash defers a trap until the
        # current FOREGROUND command finishes, so a SIGTERM arriving during
        # `sleep 20` is handled up to 20 seconds late. The `read` builtin
        # returns immediately when a trapped signal arrives, which is what
        # makes ^C feel instant instead of feeling hung.
        read -r -t "$FLEET_POLL_SECONDS" -u 3 _ 2>/dev/null || true
    done
    return 0
}

shutdown_children() {
    # SIGTERM first: the dispatcher's own handler force-kills the worker
    # process groups it tracks, so signalling it is strictly better than
    # hunting workers ourselves.
    local i pid deadline
    for i in "${!TIER_TAG[@]}"; do
        pid=${TIER_PID[i]}
        # The pattern re-check is the PID-REUSE GUARD. Between the last poll
        # and now the tier may have died and its pid been handed to something
        # unrelated; killing that would be the launcher causing the very kind
        # of collateral damage it exists to prevent.
        if pid_matches "$pid" "${TIER_PATTERN[i]}"; then
            log "fleet-up" "SIGTERM ${TIER_TAG[i]} (pid $pid)"
            kill -TERM "$pid" 2>/dev/null || true
        fi
        TIER_STATE[i]=stopped
    done
    deadline=$(( $(now_epoch) + FLEET_GRACE_SECONDS ))
    while [ "$(now_epoch)" -lt "$deadline" ]; do
        local remaining=0
        for i in "${!TIER_TAG[@]}"; do
            pid_matches "${TIER_PID[i]}" "${TIER_PATTERN[i]}" && remaining=$((remaining + 1))
        done
        [ "$remaining" -eq 0 ] && break
        sleep 1
    done
    for i in "${!TIER_TAG[@]}"; do
        if pid_matches "${TIER_PID[i]}" "${TIER_PATTERN[i]}"; then
            log "fleet-up" "SIGKILL ${TIER_TAG[i]} (pid ${TIER_PID[i]}) -- did not exit in ${FLEET_GRACE_SECONDS}s"
            kill -KILL "${TIER_PID[i]}" 2>/dev/null || true
        fi
    done
}

on_err() {
    # `set -e` terminates SILENTLY. That is how this supervisor vanished on
    # 2026-07-30 at 23:40:43 -- an ENOSPC `mv` inside write_state, no log
    # line, no exit message, 11 dead mergers left unsupervised for an hour
    # while the state file it failed to update still said "running".
    #
    # This trap costs nothing when nothing is wrong and converts any future
    # unguarded failure from "the fleet quietly stopped" into a line naming
    # the command, the line number and the exit code. It is deliberately the
    # LAST thing that runs, so it must not itself be able to fail: every
    # command in here is guarded.
    local rc=$? cmd=${BASH_COMMAND:-?} line=${BASH_LINENO[0]:-?}
    [ "$SHUTTING_DOWN" -eq 1 ] && return 0
    log "fleet-up" "FATAL: supervisor dying on an unguarded failure --" \
        "rc=$rc at ${BASH_SOURCE[0]:-fleet_up.sh}:$line: $cmd" || true
    log "fleet-up" "FATAL: $(disk_free_human 2>/dev/null || true)." \
        "Tiers this launcher started are now ORPHANED and unsupervised;" \
        "re-run $0 to re-adopt them, or --down to stop them." || true
    return 0
}

on_signal() {
    # Idempotent: a second ^C while we are already draining must not restart
    # the drain from the top.
    [ "$SHUTTING_DOWN" -eq 1 ] && return
    SHUTTING_DOWN=1
    log "fleet-up" "signal received -- shutting the fleet down"
    shutdown_children
    # Order matters: write_state RE-CREATES the pidfile (it is the one place
    # that writes it), so removing the pidfile first and then writing state
    # would leave a pidfile pointing at a supervisor that is about to exit --
    # and the next --status would report a live fleet that is not there.
    # Both files go, so --status after a shutdown reads the same as --status
    # after --down: nothing running.
    rm -f "$FLEET_PIDFILE" "$FLEET_STATEFILE"
    log "fleet-up" "fleet down"
    exit 0
}

# ---------------------------------------------------------------------------
# Subcommands
# ---------------------------------------------------------------------------

read_state() {
    [ -f "$FLEET_STATEFILE" ] || return 1
    cat "$FLEET_STATEFILE"
}

cmd_scale() {
    # Resize a RUNNING fleet without a full down/up. Writes the desired count;
    # the supervisor picks it up on its next poll and restarts only the
    # dispatcher. Without this, changing the worker count meant --down + --up,
    # which throws away every in-flight worker and the mergers' batch state
    # too -- and, on a launchd-managed fleet, silently did nothing at all
    # unless you also knew to bootout+bootstrap (kickstart re-runs the OLD
    # cached spec).
    local want=$1
    case "$want" in
        ''|*[!0-9]*) printf 'fleet_up.sh: --scale needs a positive integer\n' >&2; return 64 ;;
    esac
    [ "$want" -gt 0 ] || { printf 'fleet_up.sh: --scale must be > 0\n' >&2; return 64; }
    mkdir -p "$(dirname "$FLEET_SCALEFILE")"
    printf '%s\n' "$want" > "$FLEET_SCALEFILE"
    if read_state >/dev/null 2>&1; then
        printf 'scale requested: %s workers -- the supervisor applies it within %ss (watch %s)\n' \
            "$want" "$FLEET_POLL_SECONDS" "$FLEET_LOG"
    else
        # Deliberately still written: --scale on a stopped fleet is how you set
        # the size the next start will use, and silently doing nothing here
        # would be the launchd trap all over again.
        printf 'scale saved: %s workers -- no fleet is running, so it takes effect on the next start\n' "$want"
    fi
}

cmd_status() {
    local state
    if ! state=$(read_state); then
        printf 'fleet: no state file at %s -- this launcher has never run (or --down cleaned up)\n' \
            "$FLEET_STATEFILE"
        return 3
    fi
    printf '%-22s %-8s %-9s %s\n' TIER PID RECORDED ACTUAL
    local tag pid recorded pattern actual rc=0
    while IFS=$'\t' read -r tag pid recorded pattern; do
        if pid_matches "$pid" "$pattern"; then
            actual=alive
        else
            actual=DEAD
            [ "$recorded" = stopped ] || rc=1
        fi
        printf '%-22s %-8s %-9s %s\n' "$tag" "$pid" "$recorded" "$actual"
    done <<<"$state"
    return $rc
}

cmd_down() {
    local state
    if ! state=$(read_state); then
        printf 'fleet: nothing to stop -- no state file at %s\n' "$FLEET_STATEFILE"
        return 0
    fi
    # Stopping the supervisor first would be wrong: it would immediately
    # restart anything we then killed. Signal it, wait for it to drain its own
    # children (its trap does exactly what we would do), and only sweep up
    # afterwards.
    local tag pid recorded pattern sup=0
    while IFS=$'\t' read -r tag pid recorded pattern; do
        [ "$tag" = supervisor ] || continue
        if pid_matches "$pid" "$pattern"; then
            log "fleet-up" "SIGTERM supervisor (pid $pid); waiting up to ${FLEET_GRACE_SECONDS}s for it to drain"
            kill -TERM "$pid" 2>/dev/null || true
            sup=$pid
        fi
    done <<<"$state"
    if [ "$sup" -ne 0 ]; then
        local deadline
        deadline=$(( $(now_epoch) + FLEET_GRACE_SECONDS ))
        while [ "$(now_epoch)" -lt "$deadline" ] && pid_matches "$sup" "fleet_up.sh"; do sleep 1; done
    fi
    local killed=0
    while IFS=$'\t' read -r tag pid recorded pattern; do
        [ "$tag" = supervisor ] && continue
        if pid_matches "$pid" "$pattern"; then
            log "fleet-up" "SIGTERM $tag (pid $pid)"
            kill -TERM "$pid" 2>/dev/null || true
            killed=$((killed + 1))
        fi
    done <<<"$state"
    if [ "$killed" -gt 0 ]; then
        sleep "$FLEET_GRACE_SECONDS"
        while IFS=$'\t' read -r tag pid recorded pattern; do
            [ "$tag" = supervisor ] && continue
            if pid_matches "$pid" "$pattern"; then
                log "fleet-up" "SIGKILL $tag (pid $pid)"
                kill -KILL "$pid" 2>/dev/null || true
            fi
        done <<<"$state"
    fi
    rm -f "$FLEET_PIDFILE" "$FLEET_STATEFILE"
    log "fleet-up" "fleet down (signalled $killed tier process(es))"
    printf 'Note: workers are killed by the dispatcher'"'"'s own SIGTERM handler.\n'
    printf 'If any survive, sweep them with: uv run scripts/stop_parallel_fix.py\n'
}

cmd_up() {
    mkdir -p "$FLEET_LOG_DIR"
    local pin_rc=0
    pin_repo "$SCRIPT_REPO" || pin_rc=$?
    if [ "$pin_rc" -eq 10 ]; then
        return 0  # dry run that stopped at the pin decision; already explained
    fi
    [ "$pin_rc" -eq 0 ] || return "$pin_rc"
    run_preflight || exit 2

    # Resolve the mergers cap now that $PINNED_CONFIG is known: CLI --mergers
    # and env $FLEET_MAX_MERGERS were already applied (both non-empty at this
    # point iff one of them fired) BEFORE config.toml could even be found, so
    # only fall through to config.toml's [fleet].mergers when NEITHER set it.
    # See the FLEET_MAX_MERGERS declaration up top for the full precedence.
    if [ -z "$FLEET_MAX_MERGERS" ]; then
        FLEET_MAX_MERGERS=$(config_mergers "$PINNED_CONFIG")
        case "$FLEET_MAX_MERGERS" in
            ''|*[!0-9]*) die "config.toml's [fleet].mergers is not a non-negative integer: " \
                "'$FLEET_MAX_MERGERS' ($PINNED_CONFIG)" ;;
        esac
    fi

    local squads squad n=0
    squads=$(parse_squads "$PINNED_CONFIG") \
        || die "cannot read [squads.*] tables from $PINNED_CONFIG"
    [ -n "$squads" ] || die "$PINNED_CONFIG defined no squads"

    # Tier 2 before tier 1: a merger that starts after the dispatcher has
    # already produced commits just picks them up on its first poll, but a
    # dispatcher running with no merger produces commits nothing consumes,
    # which is the shape of the original outage.
    while read -r squad; do
        [ -n "$squad" ] || continue
        if [ "$FLEET_MAX_MERGERS" -gt 0 ] && [ "$n" -ge "$FLEET_MAX_MERGERS" ]; then
            break
        fi
        tier_add "merger:$squad" merger "$squad" "squad_merge_loop.py --squad $squad "
        n=$((n + 1))
    done <<<"$squads"
    if [ "$FLEET_MAX_MERGERS" -gt 0 ]; then
        local total_squads
        total_squads=$(printf '%s\n' "$squads" | grep -c .)
        if [ "$n" -lt "$total_squads" ]; then
            log "fleet-up" "WARNING: mergers cap $FLEET_MAX_MERGERS covers $n of $total_squads squads --" \
                "the rest are not being merged this run"
        fi
    fi
    tier_add "dispatcher" dispatcher "" "parallel_model_fix_loop.py"
    if [ "$WITH_JUDGMENT" -eq 1 ]; then
        tier_add "judgment" judgment "" "judgment_queue_daemon.py"
        log "fleet-up" "tiers: $n mergers + dispatcher(${FLEET_WORKERS} workers, squad_mode=${FLEET_SQUAD_MODE}) + judgment queue"
    else
        log "fleet-up" "tiers: $n mergers + dispatcher(${FLEET_WORKERS} workers)." \
            "WARNING: --no-judgment -- quarantined commits will accumulate in" \
            "$FLEET_LOG_DIR/quarantine.jsonl with nothing reading them."
    fi

    if [ "$DRY_RUN" -eq 1 ]; then
        printf '\nDRY RUN -- nothing started, no worktree touched.\n'
        printf '  repo    : %s (branch %s)\n' "$PINNED_REPO" "$(repo_branch "$PINNED_REPO")"
        printf '  config  : %s\n' "$PINNED_CONFIG"
        printf '  perl-lib: %s\n' "$FLEET_PERL_LIB"
        printf '  log     : %s\n' "$FLEET_LOG"
        local i
        for i in "${!TIER_TAG[@]}"; do printf '  tier    : %s\n' "${TIER_TAG[i]}"; done
        return 0
    fi

    sync_worktrees "$FLEET_WORKTREE_BASE"

    trap on_signal INT TERM
    trap on_err ERR
    # fd 3 is the interruptible-sleep channel for `read -t` in supervise().
    # It MUST be a fifo we hold open read-write and never write to: reading
    # /dev/null returns EOF instantly, so `read -t` would return immediately
    # instead of waiting and the supervisor would spin a core at 100%. The
    # fifo is unlinked right after opening -- the fd stays valid, and nothing
    # is left behind if we are killed.
    local fifo="$FLEET_LOG_DIR/.fleet-up-tick.$$"
    rm -f "$fifo"
    mkfifo "$fifo" || die "cannot create tick fifo $fifo"
    exec 3<>"$fifo"
    rm -f "$fifo"

    local i
    for i in "${!TIER_TAG[@]}"; do tier_start "$i"; done
    write_state
    # Seed the scale file with the size we actually started at. Without this a
    # stale request from a previous run -- or from a `--scale` issued while the
    # fleet was down -- would fire on the first tick and pointlessly bounce a
    # dispatcher that is already the right size.
    printf '%s\n' "$FLEET_WORKERS" > "$FLEET_SCALEFILE" 2>/dev/null || true
    log "fleet-up" "fleet up. consolidated log: $FLEET_LOG ; state: $FLEET_STATEFILE"
    supervise
}

usage() {
    sed -n '3,60p' "$0" | sed 's/^# \{0,1\}//'
}

# ---------------------------------------------------------------------------
# Entry point. Sourcing with FLEET_UP_SOURCE_ONLY=1 defines every function and
# runs nothing, which is what makes scripts/test_fleet_up.py able to unit-test
# the salvage/liveness/backoff logic against real tempdir git repos without a
# live fleet.
# ---------------------------------------------------------------------------

SCRIPT_REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
DRY_RUN=0
WITH_JUDGMENT=1
SCALE_TO=

main() {
    local action=up
    while [ $# -gt 0 ]; do
        case "$1" in
            --status) action=status ;;
            --down|--stop) action=down ;;
            --scale) action=scale; SCALE_TO=${2:?--scale needs a number}; shift ;;
            --scale=*) action=scale; SCALE_TO=${1#*=} ;;
            --dry-run) DRY_RUN=1 ;;
            --no-judgment) WITH_JUDGMENT=0 ;;
            --squad-mode) FLEET_SQUAD_MODE=1 ;;
            --workers) FLEET_WORKERS=${2:?--workers needs a number}; shift ;;
            --workers=*) FLEET_WORKERS=${1#*=} ;;
            --mergers) FLEET_MAX_MERGERS=${2:?--mergers needs a number}; shift ;;
            --mergers=*) FLEET_MAX_MERGERS=${1#*=} ;;
            --repo) FLEET_REPO=${2:?--repo needs a path}; shift ;;
            --repo=*) FLEET_REPO=${1#*=} ;;
            --config) FLEET_CONFIG=${2:?--config needs a path}; shift ;;
            --config=*) FLEET_CONFIG=${1#*=} ;;
            --perl-lib) FLEET_PERL_LIB=${2:?--perl-lib needs a path}; shift ;;
            --perl-lib=*) FLEET_PERL_LIB=${1#*=} ;;
            -h|--help) usage; return 0 ;;
            *) printf 'fleet_up.sh: unknown argument %s (try --help)\n' "$1" >&2; return 64 ;;
        esac
        shift
    done
    case "$FLEET_WORKERS" in
        ''|*[!0-9]*) printf 'fleet_up.sh: --workers must be a positive integer\n' >&2; return 64 ;;
    esac
    [ "$FLEET_WORKERS" -gt 0 ] || { printf 'fleet_up.sh: --workers must be > 0\n' >&2; return 64; }
    case "$FLEET_MAX_MERGERS" in
        '') ;;  # unset: cmd_up resolves it from config.toml's [fleet].mergers, default 0
        *[!0-9]*) printf 'fleet_up.sh: --mergers must be a non-negative integer\n' >&2; return 64 ;;
    esac
    case "$FLEET_SQUAD_MODE" in
        0|1) ;;
        *) printf 'fleet_up.sh: FLEET_SQUAD_MODE must be 0 or 1\n' >&2; return 64 ;;
    esac
    case "$action" in
        status) cmd_status ;;
        down)   cmd_down ;;
        scale)  cmd_scale "$SCALE_TO" ;;
        up)     cmd_up ;;
    esac
}

if [ "${FLEET_UP_SOURCE_ONLY:-0}" != "1" ]; then
    main "$@"
fi
