#!/usr/bin/env bash
# restore-oracle.sh — rebuild /tmp/oxidex-exiftool-cache from the i7 fleet
# host after macOS purges /tmp (this has happened three times now; see
# AGENTS.md "Never grade against an unpinned ExifTool").
#
# The i7 (ssh allen@server) is the only host in the fleet with a verified-good
# oracle right now. This script pulls its cache over rsync, then re-runs both
# capability probes locally so a bad copy (or a bad local perl) fails loudly
# instead of producing confident, wrong numbers later.
#
# Usage:
#   tools/restore-oracle.sh                # full restore (checkout + corpora)
#   tools/restore-oracle.sh --minimal       # exiftool checkout + t/images only
#   ORACLE_HOST=user@host tools/restore-oracle.sh   # override the remote
#
# Idempotent: safe to re-run any time; rsync only transfers what changed, and
# the verification always re-runs against whatever ends up on disk.

set -euo pipefail

ORACLE_HOST="${ORACLE_HOST:-allen@server}"
REMOTE_DIR="${REMOTE_DIR:-/tmp/oxidex-exiftool-cache}"
LOCAL_DIR="${LOCAL_DIR:-/tmp/oxidex-exiftool-cache}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PINNED_VERSION_FILE="${REPO_ROOT}/.exiftool-version"
MODE="full"
# Above this remote total size (MB) we refuse a full sync unless --full is
# forced, and fall back to the checkout + t/images only.
MINIMAL_THRESHOLD_MB=2000

for arg in "$@"; do
  case "$arg" in
    --minimal) MODE="minimal" ;;
    --full) MODE="full-forced" ;;
    -h|--help)
      sed -n '2,20p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *)
      echo "restore-oracle.sh: unrecognized argument: $arg" >&2
      exit 2
      ;;
  esac
done

die() {
  echo "restore-oracle.sh: FATAL: $*" >&2
  exit 1
}

echo "=== restore-oracle: instrument ==="
echo "remote host   : ${ORACLE_HOST}"
echo "remote dir    : ${REMOTE_DIR}"
echo "local dir     : ${LOCAL_DIR}"
echo "requested mode: ${MODE}"
if [ -f "${PINNED_VERSION_FILE}" ]; then
  EXPECTED_VERSION="$(tr -d '[:space:]' < "${PINNED_VERSION_FILE}")"
  echo "pinned version (from ${PINNED_VERSION_FILE}): ${EXPECTED_VERSION}"
else
  EXPECTED_VERSION="13.59"
  echo "WARNING: ${PINNED_VERSION_FILE} not found; falling back to hardcoded ${EXPECTED_VERSION}"
fi
echo "==================================="

# --- 1. Reachability ---------------------------------------------------
if ! ssh -o BatchMode=yes -o ConnectTimeout=10 "${ORACLE_HOST}" 'true' 2>/tmp/restore-oracle-ssh-err.$$; then
  cat /tmp/restore-oracle-ssh-err.$$ >&2
  rm -f /tmp/restore-oracle-ssh-err.$$
  die "cannot reach ${ORACLE_HOST} over ssh. Is the i7 up? (the ryzen is removed from the fleet; the i7 is the only oracle host)"
fi
rm -f /tmp/restore-oracle-ssh-err.$$

if ! ssh -o BatchMode=yes -o ConnectTimeout=10 "${ORACLE_HOST}" "test -d '${REMOTE_DIR}'"; then
  die "${ORACLE_HOST}:${REMOTE_DIR} does not exist. The i7's own cache may need restoring first."
fi

# --- 2. Decide full vs minimal by remote size ---------------------------
REMOTE_SIZE_MB="$(ssh -o BatchMode=yes "${ORACLE_HOST}" "du -sm '${REMOTE_DIR}' 2>/dev/null | cut -f1")"
echo "remote cache size: ${REMOTE_SIZE_MB} MB"

if [ "${MODE}" = "full" ] && [ "${REMOTE_SIZE_MB}" -gt "${MINIMAL_THRESHOLD_MB}" ]; then
  echo "NOTE: remote cache (${REMOTE_SIZE_MB} MB) exceeds ${MINIMAL_THRESHOLD_MB} MB threshold;"
  echo "      pulling exiftool checkout + t/images only. Pass --full to force everything."
  MODE="minimal"
fi

mkdir -p "${LOCAL_DIR}"

# --- 3. Transfer ---------------------------------------------------------
if [ "${MODE}" = "minimal" ]; then
  echo "--- rsync: minimal (exiftool checkout + pinned script + version file) ---"
  rsync -az --delete \
    "${ORACLE_HOST}:${REMOTE_DIR}/exiftool/" "${LOCAL_DIR}/exiftool/"
  rsync -az \
    "${ORACLE_HOST}:${REMOTE_DIR}/exiftool-pinned.sh" "${LOCAL_DIR}/exiftool-pinned.sh"
  rsync -az \
    "${ORACLE_HOST}:${REMOTE_DIR}/.exiftool-version" "${LOCAL_DIR}/.exiftool-version"
  echo "SAID SO: sample corpora (combined-samples/, s-*.tgz) were NOT copied — remote cache is ${REMOTE_SIZE_MB} MB."
else
  echo "--- rsync: full cache (checkout + pinned script + version file + sample corpora) ---"
  rsync -az "${ORACLE_HOST}:${REMOTE_DIR}/" "${LOCAL_DIR}/"
fi

chmod +x "${LOCAL_DIR}/exiftool-pinned.sh" 2>/dev/null || true

# --- 4. Verify: -ver ------------------------------------------------------
[ -x "${LOCAL_DIR}/exiftool-pinned.sh" ] || die "exiftool-pinned.sh missing or not executable after sync at ${LOCAL_DIR}/exiftool-pinned.sh"

ACTUAL_VERSION="$("${LOCAL_DIR}/exiftool-pinned.sh" -ver 2>/tmp/restore-oracle-ver-err.$$)" || {
  echo "--- exiftool-pinned.sh -ver failed; stderr: ---" >&2
  cat /tmp/restore-oracle-ver-err.$$ >&2
  rm -f /tmp/restore-oracle-ver-err.$$
  die "exiftool-pinned.sh -ver did not run. Check local perl and the synced exiftool/lib tree."
}
rm -f /tmp/restore-oracle-ver-err.$$

echo "-ver reports: ${ACTUAL_VERSION}"
if [ "${ACTUAL_VERSION}" != "${EXPECTED_VERSION}" ]; then
  die "-ver mismatch: got '${ACTUAL_VERSION}', expected '${EXPECTED_VERSION}' (from ${PINNED_VERSION_FILE}). Do not use this oracle for measurements."
fi

# --- 5. Verify: capability probe (DOCX via Archive::Zip) ------------------
DOCX_SAMPLE="${LOCAL_DIR}/exiftool/t/images/OOXML.docx"
[ -f "${DOCX_SAMPLE}" ] || die "capability-probe fixture missing: ${DOCX_SAMPLE} (exiftool/t/images did not sync correctly)"

DOCX_RESULT="$("${LOCAL_DIR}/exiftool-pinned.sh" -s3 -FileType "${DOCX_SAMPLE}" 2>/tmp/restore-oracle-docx-err.$$)" || {
  echo "--- capability probe failed to run; stderr: ---" >&2
  cat /tmp/restore-oracle-docx-err.$$ >&2
  rm -f /tmp/restore-oracle-docx-err.$$
  die "capability probe (-s3 -FileType on OOXML.docx) errored out."
}
STDERR_CONTENT="$(cat /tmp/restore-oracle-docx-err.$$ 2>/dev/null || true)"
rm -f /tmp/restore-oracle-docx-err.$$

echo "capability probe (-s3 -FileType OOXML.docx) reports: ${DOCX_RESULT}"
if [ "${DOCX_RESULT}" != "DOCX" ]; then
  echo "This means the local perl's exiftool degraded the container-format check" >&2
  echo "(classic symptom: system perl lacks Archive::Zip, so ZIP-based containers" >&2
  echo "like docx/xlsx/pptx/etc. all misreport as FileType: ZIP while -ver still" >&2
  echo "prints the right release). Checking for the specific module now:" >&2
  if ! perl -MArchive::Zip -e 1 2>/tmp/restore-oracle-archzip-err.$$; then
    echo "--- perl -MArchive::Zip failed: ---" >&2
    cat /tmp/restore-oracle-archzip-err.$$ >&2
    rm -f /tmp/restore-oracle-archzip-err.$$
    die "local perl is missing Archive::Zip (or another required module). Install it for the perl that runs '${LOCAL_DIR}/exiftool-pinned.sh' (system perl at $(command -v perl)), e.g.: cpan Archive::Zip  -- then re-run this script. Do NOT treat a matching -ver as a working oracle."
  fi
  rm -f /tmp/restore-oracle-archzip-err.$$
  die "capability probe reported '${DOCX_RESULT}' instead of DOCX, but Archive::Zip is present locally — investigate before trusting this oracle for measurements. stderr was: ${STDERR_CONTENT}"
fi

# --- 6. Report -------------------------------------------------------------
echo "==================================="
echo "restore-oracle: OK"
echo "  -ver            : ${ACTUAL_VERSION} (matches pin)"
echo "  capability probe: DOCX (Archive::Zip present, container detection sane)"
echo "  cache dir       : ${LOCAL_DIR} ($(du -sh "${LOCAL_DIR}" 2>/dev/null | cut -f1))"
echo "  mode            : ${MODE}"
echo "==================================="
