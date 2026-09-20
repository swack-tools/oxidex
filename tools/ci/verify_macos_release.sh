#!/usr/bin/env bash
set -euo pipefail

: "${ASSET_DIR:?missing ASSET_DIR}"
: "${VERSION:?missing VERSION}"
: "${DEVELOPMENT_TEAM:?missing DEVELOPMENT_TEAM}"
: "${RUNNER_TEMP:?missing RUNNER_TEMP}"

MAC_BIN="$ASSET_DIR/oxidex-universal-apple-darwin"
DMG="$ASSET_DIR/oxidex-v${VERSION}.dmg"
test -s "$MAC_BIN"
test -s "$DMG"
chmod u+x "$MAC_BIN"

MOUNT_DIR=$(mktemp -d "$RUNNER_TEMP/oxidex-dmg-mount.XXXXXX")
ATTACHED=0
cleanup_mount() {
  local verify_exit=$?
  trap - EXIT HUP INT TERM
  if [ "$ATTACHED" -eq 1 ]; then
    if ! hdiutil detach "$MOUNT_DIR"; then
      verify_exit=1
    fi
  fi
  if ! rmdir "$MOUNT_DIR"; then
    verify_exit=1
  fi
  exit "$verify_exit"
}
trap cleanup_mount EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

verify_signature() {
  local executable=$1
  local details
  codesign --verify --strict --verbose=4 "$executable"
  details=$(codesign --display --verbose=4 "$executable" 2>&1)
  printf '%s\n' "$details"
  printf '%s\n' "$details" | grep -F "TeamIdentifier=$DEVELOPMENT_TEAM" >/dev/null
  printf '%s\n' "$details" | grep -F 'Runtime Version=' >/dev/null
  printf '%s\n' "$details" | grep -F 'Timestamp=' >/dev/null
}

verify_universal_architectures() {
  local executable=$1
  local archs normalized_archs
  archs=$(lipo -archs "$executable")
  normalized_archs=$(printf '%s\n' "$archs" \
    | awk '{ for (i = 1; i <= NF; i++) print $i }' \
    | LC_ALL=C sort | paste -sd ' ' -)
  if [ "$normalized_archs" != "arm64 x86_64" ]; then
    printf 'refusing non-universal macOS executable %s: expected exactly arm64 x86_64, got %s\n' \
      "$executable" "$archs" >&2
    return 1
  fi
}

verify_universal_architectures "$MAC_BIN"
verify_signature "$MAC_BIN"
spctl --assess --type execute --verbose=4 "$MAC_BIN"
test "$("$MAC_BIN" --version)" = "oxidex $VERSION"
"$MAC_BIN" --help >/dev/null

xcrun stapler validate -v "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG"

hdiutil attach -readonly -nobrowse -noautoopen -plist \
  -mountpoint "$MOUNT_DIR" "$DMG" >/dev/null
ATTACHED=1
find "$MOUNT_DIR" -type f -name oxidex -print > "$RUNNER_TEMP/oxidex-payload-paths.txt"
test "$(wc -l < "$RUNNER_TEMP/oxidex-payload-paths.txt" | tr -d '[:space:]')" -eq 1
IFS= read -r MAC_DMG_PAYLOAD < "$RUNNER_TEMP/oxidex-payload-paths.txt"
test "$MAC_DMG_PAYLOAD" = "$MOUNT_DIR/oxidex"
test -x "$MAC_DMG_PAYLOAD"
cmp "$MAC_BIN" "$MAC_DMG_PAYLOAD"

verify_universal_architectures "$MAC_DMG_PAYLOAD"
verify_signature "$MAC_DMG_PAYLOAD"
spctl --assess --type execute --verbose=4 "$MAC_DMG_PAYLOAD"
test "$("$MAC_DMG_PAYLOAD" --version)" = "oxidex $VERSION"
"$MAC_DMG_PAYLOAD" --help >/dev/null
