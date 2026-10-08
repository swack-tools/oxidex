#!/usr/bin/env bash
# Attach only the selected commit's authenticated history to an already
# extracted archive, so VitePress lastUpdated sees the same Git-backed source.
set -euo pipefail

if [ "$#" -ne 3 ] || [[ ! "$2" =~ ^[0-9a-f]{40}$ ]]; then
  echo 'usage: attach-snapshot-history.sh REPO COMMIT_SHA SNAPSHOT' >&2
  exit 64
fi
REPO="$1"
SHA="$2"
SNAPSHOT="$3"
if [ ! -d "$SNAPSHOT" ] || [ -e "$SNAPSHOT/.git" ] || [ -L "$SNAPSHOT/.git" ]; then
  echo 'snapshot history requires a fresh extracted directory' >&2
  exit 1
fi
# The remote worker already supplies a minimal child environment. Keep the
# history path independent of replace refs, machine attributes and Git config
# for direct use of the local mirror as well.
export GIT_NO_REPLACE_OBJECTS=1 GIT_ATTR_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null

git -C "$SNAPSHOT" init -q --template=
git -C "$SNAPSHOT" fetch -q --no-tags "$REPO" "$SHA"
git -C "$SNAPSHOT" update-ref refs/heads/docs-snapshot "$SHA"
git -C "$SNAPSHOT" symbolic-ref HEAD refs/heads/docs-snapshot
git -C "$SNAPSHOT" read-tree "$SHA"
if [ "$(git -C "$SNAPSHOT" rev-parse HEAD)" != "$SHA" ] || \
   [ "$(git -C "$SNAPSHOT" rev-parse 'HEAD^{tree}')" != "$(git -C "$REPO" rev-parse "$SHA^{tree}")" ] || \
   [ -n "$(git -C "$SNAPSHOT" status --porcelain --untracked-files=all)" ]; then
  echo 'snapshot history does not match the extracted commit' >&2
  exit 1
fi
