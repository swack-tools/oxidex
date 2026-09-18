#!/usr/bin/env bash
# preflight.sh -- answer "where am I, and may I write here?" before the first edit,
# and (with flags) "can I reach what I am about to touch?" before remote work.
#
# WHY THIS EXISTS. Two failures this repo has actually paid for:
#   * Several agents editing one shared worktree at once: a live acceptance run
#     found its tree gone dirty 58 s in, from a sibling's staged edits. Anything
#     measured after that point was measuring an unknown tree (see AGENTS.md's
#     instrument-failure list, and `scripts/instrument.py`'s dirty-tree refusal,
#     which this complements: that one guards MEASUREMENT, this guards EDITING).
#   * Work started against a stale base, so a "fix" raced an upstream that
#     already had it.
# Exit non-zero and say why, rather than letting either proceed silently.
#
# USAGE
#   tools/preflight.sh                     # worktree + branch + cleanliness
#   tools/preflight.sh --upstream          # ... plus base freshness vs origin
#   tools/preflight.sh --host allen@server # ... plus ssh reachability
#   tools/preflight.sh --github            # ... plus gh identity + push scope
#   tools/preflight.sh --k8s               # ... plus current kube context
#   tools/preflight.sh --all --host X
#
# Exit codes: 0 ok | 2 protected branch | 3 dirty tree | 4 stale base
#             5 remote unreachable | 64 usage
set -uo pipefail

# Branches only the maintainer lands. An agent that finds itself on one has
# already lost track of its worktree; refuse before the first edit, not after.
PROTECTED_DEFAULT="main refactor/tag-machinery"
PROTECTED="${PREFLIGHT_PROTECTED_BRANCHES:-$PROTECTED_DEFAULT}"

CHECK_UPSTREAM=0 CHECK_GH=0 CHECK_K8S=0 HOSTS=() RC=0
while [ $# -gt 0 ]; do
  case "$1" in
    --upstream) CHECK_UPSTREAM=1 ;;
    --github)   CHECK_GH=1 ;;
    --k8s)      CHECK_K8S=1 ;;
    --host)     shift; [ $# -gt 0 ] || { echo "preflight: --host needs a value" >&2; exit 64; }; HOSTS+=("$1") ;;
    --all)      CHECK_UPSTREAM=1; CHECK_GH=1; CHECK_K8S=1 ;;
    -h|--help)  sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "preflight: unknown argument '$1'" >&2; exit 64 ;;
  esac
  shift
done

say() { printf '%s\n' "$*"; }
fail() { printf 'preflight: %s\n' "$*" >&2; }

# --- identity of this checkout -------------------------------------------
ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || { fail "not inside a git repository"; exit 64; }
COMMON=$(git rev-parse --git-common-dir 2>/dev/null)
GITDIR=$(git rev-parse --git-dir 2>/dev/null)
# A linked worktree's --git-dir sits under the main .git; equal paths mean this
# IS the main checkout.
if [ "$(cd "$GITDIR" && pwd -P)" = "$(cd "$COMMON" && pwd -P)" ]; then KIND="main checkout"; else KIND="linked worktree"; fi
BRANCH=$(git symbolic-ref --quiet --short HEAD 2>/dev/null || echo "(detached)")
HEAD_SHA=$(git rev-parse --short HEAD 2>/dev/null || echo "?")
DIRTY=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')

say "worktree : $ROOT  [$KIND]"
say "branch   : $BRANCH @ $HEAD_SHA"
say "dirty    : $DIRTY file(s)"

for p in $PROTECTED; do
  if [ "$BRANCH" = "$p" ]; then
    fail "branch '$BRANCH' is protected -- create a worktree for your own branch:"
    fail "  git -C $ROOT worktree add -b <branch> /Users/allen/git/<dir> <base>"
    RC=2
  fi
done

if [ "$DIRTY" != "0" ]; then
  fail "$DIRTY uncommitted file(s) -- another agent may share this tree, or a prior run left residue."
  fail "  inspect with: git -C $ROOT status --short"
  [ "$RC" = "0" ] && RC=3
fi

# --- base freshness ------------------------------------------------------
if [ "$CHECK_UPSTREAM" = "1" ]; then
  if git fetch -q origin 2>/dev/null; then
    for up in origin/main origin/refactor/tag-machinery; do
      git rev-parse --verify --quiet "$up" >/dev/null || continue
      BEHIND=$(git rev-list --count "HEAD..$up" 2>/dev/null || echo 0)
      say "base     : $BEHIND commit(s) behind $up"
      if [ "${BEHIND:-0}" -gt 0 ]; then
        fail "HEAD is behind $up -- re-verify the issue still reproduces at the current base,"
        fail "  and check whether upstream already contains your fix (then report it superseded)."
        [ "$RC" = "0" ] && RC=4
      fi
    done
  else
    fail "could not fetch origin -- base freshness UNVERIFIED (not the same as fresh)"
    [ "$RC" = "0" ] && RC=5
  fi
fi

# --- reachability of what you are about to touch -------------------------
for h in "${HOSTS[@]:-}"; do
  [ -n "$h" ] || continue
  if ssh -o BatchMode=yes -o ConnectTimeout=8 "$h" true 2>/dev/null; then
    say "ssh      : $h reachable"
  else
    fail "ssh $h UNREACHABLE -- do not start remote work you cannot verify"
    [ "$RC" = "0" ] && RC=5
  fi
done

if [ "$CHECK_GH" = "1" ]; then
  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    say "github   : $(gh api user --jq .login 2>/dev/null || echo '(identity unread)')"
  else
    fail "gh not authenticated -- GitHub identity UNVERIFIED"
    [ "$RC" = "0" ] && RC=5
  fi
fi

if [ "$CHECK_K8S" = "1" ]; then
  if command -v kubectl >/dev/null 2>&1; then
    CTX=$(kubectl config current-context 2>/dev/null || echo "(none)")
    say "k8s ctx  : $CTX"
    [ "$CTX" = "(none)" ] && { fail "no current kube context -- refuse cluster work until one is chosen"; [ "$RC" = "0" ] && RC=5; }
  else
    say "k8s ctx  : kubectl not installed (skipped)"
  fi
fi

[ "$RC" = "0" ] && say "preflight: OK"
exit "$RC"
