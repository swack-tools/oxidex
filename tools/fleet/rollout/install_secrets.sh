#!/usr/bin/env bash
# install_secrets.sh -- create/validate the git-token half of the Keel
# secrets bundle (SPEC 8 "Secrets bundle"), for the HTTPS credential-helper
# path (`tools/fleet/keel/git-credential-file`, `FLEET_GIT_TOKEN_FILE`).
#
# WHY THIS EXISTS (B5, review finding). No unit/plist/cron template was
# setting FLEET_GIT_TOKEN_FILE at all. With the state repo now private
# HTTPS, that means fleetd's very first git op -- the host-singleton
# claim, before any heartbeat -- raises an uncaught HubUnreachableError on
# every host without the PAT already in place by some other means, and
# `gate.sh`'s `store_verdict` fails the SAME way but silently, logged only
# as "non-fatal". This script is the fix for "how does a token correctly
# get there in the first place", checked the same way doctor.py's new
# `check_git_token_file` check re-verifies it later.
#
# WHAT THIS DOES, AND DOES NOT, DO.
#   1. Creates ~/.keel/secrets (mode 0700) if it does not exist, and
#      fixes the mode if it exists but is looser than 0700.
#   2. Validates a token file ALREADY PLACED there by a human: must
#      exist, mode exactly 0600, non-empty, and pass a real
#      `git ls-remote <state-url> HEAD` using it -- through the SAME
#      credential-helper path fleetd itself uses, never a token-in-URL
#      and never GIT_ASKPASS.
#   This script never asks for, generates, or writes the token itself
#   (SPEC 8: "Secrets bundle ... distributed once, rotated by hand") --
#   it is a `chmod`+`mkdir`+validate tool, not an enrollment flow. The
#   token is NEVER printed, logged, or placed in an argv/env value wider
#   than the one git subprocess's own environment; every diagnostic below
#   names a PATH or an exit code, never file contents.
#
# USAGE.
#   tools/fleet/rollout/install_secrets.sh --state-url <https-url> [--token-file <path>]
#   FLEET_HUB_URL=<https-url> tools/fleet/rollout/install_secrets.sh
#
# --token-file defaults to $HOME/.keel/secrets/git-token -- the exact path
# every template in tools/fleet/units/ sets FLEET_GIT_TOKEN_FILE to
# (systemd's %h and launchd's literal both resolve there; see that
# directory's own tests, test_units_secrets.py). --state-url has NO
# default (AGENTS.md "never approximate"): pass it, or set FLEET_HUB_URL,
# or the script refuses rather than validating against a guessed repo.
#
# EXIT CODES: 0 = ready; 1 = usage error; 2 = secrets dir or credential
# helper problem; 3 = token file missing (message explains how to place
# one); 4 = wrong mode; 5 = empty; 6 = the ls-remote probe failed (bad,
# expired, or under-scoped token, or a real network problem -- git's own
# stderr is shown, which never contains the token).

set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: install_secrets.sh --state-url <https-url> [--token-file <path>]
       FLEET_HUB_URL=<https-url> install_secrets.sh [--token-file <path>]

Creates ~/.keel/secrets (0700) and validates the git-token file already
placed there by hand: must exist, mode 0600, non-empty, and pass a real
`git ls-remote <state-url> HEAD`. Never prints the token.
EOF
}

state_url="${FLEET_HUB_URL:-}"
token_file="${FLEET_GIT_TOKEN_FILE:-$HOME/.keel/secrets/git-token}"

while [ $# -gt 0 ]; do
  case "$1" in
    --state-url)
      state_url="${2:-}"
      shift 2
      ;;
    --token-file)
      token_file="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "install_secrets.sh: unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [ -z "$state_url" ]; then
  echo "install_secrets.sh: no state repo URL -- pass --state-url or set FLEET_HUB_URL" >&2
  usage
  exit 1
fi

case "$state_url" in
  https://*) ;;
  *)
    echo "install_secrets.sh: --state-url must be an https:// URL (got: $state_url) -- this credential path is HTTPS-only, ssh remotes don't use it" >&2
    exit 1
    ;;
esac

secrets_dir="$(dirname "$token_file")"

# --- 1. secrets directory: create or fix mode -----------------------
if [ ! -d "$secrets_dir" ]; then
  mkdir -p "$secrets_dir" || { echo "install_secrets.sh: could not create $secrets_dir" >&2; exit 2; }
  echo "install_secrets.sh: created $secrets_dir"
fi
chmod 0700 "$secrets_dir" || { echo "install_secrets.sh: could not chmod 0700 $secrets_dir" >&2; exit 2; }

# --- 2. token file: must already exist, placed by hand ---------------
if [ ! -e "$token_file" ]; then
  cat >&2 <<EOF
install_secrets.sh: no token file at $token_file

This script does not create or fetch a token -- SPEC 8 ("Secrets
bundle"): the state-repo PAT is distributed once and rotated by hand.
Create a fine-grained GitHub PAT scoped to the state repo (contents:
write) and put JUST the token on the first line:

  umask 077
  printf '%s\n' '<paste the token here>' > "$token_file"
  chmod 0600 "$token_file"

Then re-run this script to validate it.
EOF
  exit 3
fi

# Portable mode read: BSD `stat -f` (macOS) then GNU `stat -c` (Linux).
mode="$(stat -f '%Lp' "$token_file" 2>/dev/null || stat -c '%a' "$token_file" 2>/dev/null || true)"
if [ -z "$mode" ]; then
  echo "install_secrets.sh: could not stat $token_file" >&2
  exit 4
fi
if [ "$mode" != "600" ]; then
  echo "install_secrets.sh: $token_file has mode $mode, expected 600 -- run: chmod 0600 \"$token_file\"" >&2
  exit 4
fi

if [ ! -s "$token_file" ]; then
  echo "install_secrets.sh: $token_file exists but is empty" >&2
  exit 5
fi

echo "install_secrets.sh: $secrets_dir is 0700, $token_file exists and is 0600 -- probing $state_url ..."

# --- 3. the real proof: an authenticated ls-remote --------------------
# Through the SAME credential-helper path fleetd uses (never a
# token-in-URL, never GIT_ASKPASS): reset git's helper list (an empty
# `credential.helper` value is git's documented way to clear it, so a
# host-level helper -- osxkeychain, the 1Password ssh agent this fleet
# has been bitten by before -- cannot answer first with the wrong
# identity), then point it at git-credential-file, which reads the token
# out of the file named by FLEET_GIT_TOKEN_FILE and hands it to git on
# its own stdout -- never as an argument, never as an environment value.
self_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
helper="$self_dir/../keel/git-credential-file"
if [ ! -x "$helper" ]; then
  echo "install_secrets.sh: credential helper missing or not executable: $helper" >&2
  exit 2
fi

stderr_file="$(mktemp)"
trap 'rm -f "$stderr_file"' EXIT

if FLEET_GIT_TOKEN_FILE="$token_file" \
   git -c credential.helper= -c "credential.helper=!'$helper'" \
       ls-remote "$state_url" HEAD >/dev/null 2>"$stderr_file"; then
  echo "install_secrets.sh: OK -- git ls-remote $state_url HEAD succeeded with this token"
  exit 0
else
  rc=$?
  echo "install_secrets.sh: git ls-remote $state_url HEAD FAILED (rc=$rc) -- token may be invalid, expired, revoked, or not scoped to this repo. git's own stderr (never contains the token):" >&2
  cat "$stderr_file" >&2
  exit 6
fi
