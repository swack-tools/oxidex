#!/usr/bin/env bash
# The docs snapshot digest: SHA-256 each file, sort names as bytes, hash the list.
set -euo pipefail
if [ "$#" -ne 1 ] || [ -L "$1" ] || [ ! -d "$1" ]; then
  echo 'usage: dist-sha256.sh DIST_DIRECTORY' >&2
  exit 64
fi
(
  cd "$1"
  unsafe=$(find . ! -type f ! -type d -print -quit)
  if [ -n "$unsafe" ]; then
    echo "dist contains a symlink or nonregular entry: $unsafe" >&2
    exit 1
  fi
  find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256 | awk '{print $1}'
)
