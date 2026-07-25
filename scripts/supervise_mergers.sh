#!/usr/bin/env bash
# Keep one squad_merge_loop.py alive per squad.
#
# Why this exists: a merger is the ONLY path from a worker's commit to a
# squad branch, and nothing in the fleet respawns one. On 2026-07-25 seven
# of fourteen mergers died within seconds of each other on a single
# unguarded CalledProcessError, stranding 68% of worker slots with no
# publish path for over an hour before a human noticed. The crash itself is
# now contained (squad_merge_loop.run_batch_check holds publication instead
# of raising), but "contained" is not "immortal" -- an OOM kill, an
# operator pkill, or any future unguarded raise still takes a daemon down
# silently. This turns that from a silent, unbounded outage into a gap of
# at most POLL seconds.
#
# Deliberately dumb: no lock file, no state, no restart budget. The mergers
# already own their own per-squad flock (squad_merge_loop.acquire_lock with
# stale-heartbeat takeover), so a redundant spawn loses the race and exits
# harmlessly -- which makes "just start one if none is running" safe to run
# from anywhere, including a second copy of this script.
#
# Usage:
#   nohup scripts/supervise_mergers.sh >> ~/.oxidex/logs/merger-supervisor.log 2>&1 &
set -uo pipefail

REPO="${REPO:-$HOME/.oxidex/worktrees/fleet-ops}"
LOGDIR="${LOGDIR:-$HOME/.oxidex/logs}"
POLL="${POLL:-30}"
PERL_LIB="${PERL_LIB:-/opt/homebrew/Cellar/exiftool/13.55/libexec/lib/perl5}"

SQUADS=(canon nikon sony-minolta xmp exif-core olympus pentax-samsung
        panasonic-leica mobile thermal sigma-c2pa ps-docs standards-appn tail)

cd "$REPO" || exit 1

ts() { date +'%Y-%m-%dT%H:%M:%S'; }

echo "$(ts) supervisor up (repo=$REPO poll=${POLL}s squads=${#SQUADS[@]})"

while :; do
  for sq in "${SQUADS[@]}"; do
    # Match the --squad value exactly so 'canon' never matches nothing and
    # 'sony-minolta' never matches 'sony-minolta-2'-style suffixes.
    if ! pgrep -f -- "squad_merge_loop.py --squad ${sq} " >/dev/null 2>&1; then
      echo "$(ts) [$sq] no merger running -- starting"
      nohup uv run scripts/squad_merge_loop.py \
        --squad "$sq" --infinite --poll-seconds 60 \
        --perl-lib "$PERL_LIB" \
        >> "$LOGDIR/merger-$sq.log" 2>&1 &
      disown
    fi
  done
  sleep "$POLL"
done
