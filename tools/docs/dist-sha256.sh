#!/usr/bin/env bash
# The docs snapshot digest: SHA-256 each file, sort names as bytes, hash the list.
set -euo pipefail
if [ "$#" -ne 1 ] || [ ! -d "$1" ]; then
  echo 'usage: dist-sha256.sh DIST_DIRECTORY' >&2
  exit 64
fi
(
  cd "$1"
  find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256 | awk '{print $1}'
)
