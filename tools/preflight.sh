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
#   * A compiler other than the pin, resolved implicitly: rust-toolchain.toml
#     is honoured only by rustup's proxies, so with /opt/homebrew/bin ahead of
#     ~/.cargo/bin on PATH, `rustc` and `cargo` are Homebrew's and every build
#     silently uses that release (all local builds and corpus measurements on
#     2026-09-23 were 1.98.1-built while CI used the pinned 1.97.1). Even
#     rustup's own cargo then runs whichever `rustc` PATH finds first.
# Exit non-zero and say why, rather than letting any of them proceed silently.
#
# USAGE
#   tools/preflight.sh                     # worktree + branch + cleanliness
#   tools/preflight.sh --upstream          # ... plus base freshness vs origin
#   tools/preflight.sh --host <host>       # ... plus ssh reachability
#   tools/preflight.sh --github            # ... plus gh identity + push scope
#   tools/preflight.sh --k8s               # ... plus current kube context
#   tools/preflight.sh --all --host X
#
# Always checked when the checkout has a rust-toolchain.toml: the compiler
# cargo would use here ($RUSTC, else `rustc` on PATH, run from the worktree
# root so a rustup proxy honours the pin) and `cargo` itself must be the
# pinned channel. A mismatch fails with exit 6; set
# OXIDEX_ALLOW_TOOLCHAIN_SKEW=1 to downgrade it to a printed warning (for work
# that builds nothing -- every binary built under the override is off-pin).
#
# Exit codes: 0 ok | 2 protected branch | 3 dirty tree | 4 stale base
#             5 remote unreachable | 6 toolchain differs from the pin
#             64 usage
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
    -h|--help)  sed -n '2,38p' "$0"; exit 0 ;;
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
    fail "  git -C $(printf '%q' "$ROOT") worktree add -b <branch> \"\${OXIDEX_WORKTREE_ROOT:-\$HOME/git}/<dir>\" <base>"
    RC=2
  fi
done

if [ "$DIRTY" != "0" ]; then
  fail "$DIRTY uncommitted file(s) -- another agent may share this tree, or a prior run left residue."
  fail "  inspect with: git -C $ROOT status --short"
  [ "$RC" = "0" ] && RC=3
fi

# --- compiler: the pin, not whatever PATH finds first ---------------------
# rustup honours rust-toolchain.toml only through its proxies; any other
# `rustc` on PATH (Homebrew's, a distro's) ignores it without a word. Ask the
# same question cargo will: $RUSTC if set, else `rustc` from PATH, run from
# the worktree root. `cargo -V` is checked too: cargo X.Y.Z ships with rustc
# X.Y.Z, and a Homebrew cargo first on PATH is the same hazard.
TOOLCHAIN_FILE=""
for f in rust-toolchain.toml rust-toolchain; do
  [ -f "$ROOT/$f" ] && { TOOLCHAIN_FILE="$ROOT/$f"; break; }
done
if [ -n "$TOOLCHAIN_FILE" ]; then
  PIN_CHANNEL=$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$TOOLCHAIN_FILE" | head -n 1)
  if [ -z "$PIN_CHANNEL" ] && [ "${TOOLCHAIN_FILE##*/}" = "rust-toolchain" ]; then
    PIN_CHANNEL=$(sed -n '/[^[:space:]]/{s/^[[:space:]]*//;s/[[:space:]]*$//;p;q;}' "$TOOLCHAIN_FILE")
  fi
  # Does release $2 satisfy channel $1? 0 yes, 1 no, 2 symbolic (unverifiable).
  release_matches() {
    case "$1" in
      *[!0-9.]*|"") return 2 ;;
    esac
    case "$1" in
      *.*.*) [ "$2" = "$1" ] ;;
      *.*)   case "$2" in "$1".*) return 0 ;; *) return 1 ;; esac ;;
      *)     return 2 ;;
    esac
  }
  RUSTC_CMD="${RUSTC:-rustc}"
  RUSTC_PATH=$(command -v "$RUSTC_CMD" 2>/dev/null || true)
  RUSTC_VV=$(cd "$ROOT" && "$RUSTC_CMD" -vV 2>/dev/null) || RUSTC_VV=""
  RUSTC_RELEASE=$(printf '%s\n' "$RUSTC_VV" | sed -n 's/^release: *//p')
  RUSTC_SYSROOT=$(cd "$ROOT" && "$RUSTC_CMD" --print sysroot 2>/dev/null) || RUSTC_SYSROOT="?"
  CARGO_PATH=$(command -v cargo 2>/dev/null || true)
  CARGO_V=$(cd "$ROOT" && cargo -V 2>/dev/null) || CARGO_V=""
  CARGO_RELEASE=$(printf '%s\n' "$CARGO_V" | awk '$1 == "cargo" {print $2; exit}')
  say "toolchain: pin ${PIN_CHANNEL:-?} (${TOOLCHAIN_FILE##*/})"
  say "rustc    : ${RUSTC_PATH:-$RUSTC_CMD (not found)} -> $(printf '%s\n' "$RUSTC_VV" | head -n 1)${RUSTC:+  [\$RUSTC]}"
  say "sysroot  : $RUSTC_SYSROOT"
  say "cargo    : ${CARGO_PATH:-cargo (not found)} -> ${CARGO_V:-?}"
  SKEW=()
  if [ -z "$PIN_CHANNEL" ]; then
    SKEW+=("cannot read the channel from ${TOOLCHAIN_FILE##*/}")
  else
    for pair in "rustc:$RUSTC_RELEASE" "cargo:$CARGO_RELEASE"; do
      tool=${pair%%:*} rel=${pair#*:}
      release_matches "$PIN_CHANNEL" "$rel"; m=$?
      if [ "$m" = "2" ]; then
        say "toolchain: channel '$PIN_CHANNEL' is symbolic -- $tool ${rel:-?} not checked against it"
      elif [ "$m" != "0" ]; then
        SKEW+=("$tool is ${rel:-UNRESOLVABLE}, pin is $PIN_CHANNEL")
      fi
    done
  fi
  if [ "${#SKEW[@]}" -gt 0 ]; then
    for s in "${SKEW[@]}"; do fail "TOOLCHAIN MISMATCH: $s"; done
    fail "  builds here would silently bypass ${TOOLCHAIN_FILE##*/}. Put rustup's proxies first:"
    fail "    export PATH=\"\$HOME/.cargo/bin:\$PATH\"     (see AGENTS.md 'Rust toolchain pin')"
    fail "  \`rustup run $PIN_CHANNEL cargo ...\` is NOT enough: that cargo still runs the first \`rustc\` on PATH."
    case "$(printf '%s' "${OXIDEX_ALLOW_TOOLCHAIN_SKEW:-}" | tr '[:upper:]' '[:lower:]')" in
      1|true)
        say "toolchain: MISMATCH OVERRIDDEN (OXIDEX_ALLOW_TOOLCHAIN_SKEW=1) -- anything built here is off-pin" ;;
      *)
        fail "  or set OXIDEX_ALLOW_TOOLCHAIN_SKEW=1 to proceed without building anything you will measure."
        [ "$RC" = "0" ] && RC=6 ;;
    esac
  fi
else
  say "toolchain: no rust-toolchain.toml (skipped)"
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
