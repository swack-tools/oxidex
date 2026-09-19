#!/bin/bash
# The genshare-probe/1 census (see README.md). Run it under the exclusive measurement lock:
# it is a measurement, and a starved host corrupts it.
# usage: census.sh <out-dir> <ctl-worktree> <probe-worktree> <token> [...]
# env: EXIFTOOL_PERL (a perl with Archive::Zip; required), CORPUS, ETDIR (pinned 13.59 tree)
# Control census = unmodified tip binary; inertness = probe (env unset) vs control binary over every corpus
# file; then one census per OXIDEX_PROBE_SILENCE token with the probe binary. set -u, not -e.
set -u
O="$1"; CTL="$2"; PRB="$3"; shift 3
unset PERL5LIB PERLLIB PERL5OPT OXIDEX_PROBE_SILENCE
: "${EXIFTOOL_PERL:?set EXIFTOOL_PERL to a perl that loads Archive::Zip}"
export EXIFTOOL_PERL; mkdir -p "$O"
CORPUS="${CORPUS:-/tmp/oxidex-exiftool-cache/combined-samples}"; ETDIR="${ETDIR:-/tmp/oxidex-exiftool-cache/exiftool}"
CBIN=$CTL/target/release/oxidex; PBIN=$PRB/target/release/oxidex
echo "=== instrument: genshare census out=$O ctl=$CTL@$(git -C $CTL rev-parse --short=8 HEAD) dirty=$(git -C $CTL status --porcelain | wc -l | tr -d ' ') probe=$PRB@$(git -C $PRB rev-parse --short=8 HEAD) dirty=$(git -C $PRB status --porcelain | wc -l | tr -d ' ') tokens=$* host=$(hostname) $(date -u +%FT%TZ)"
echo "files=$(find $CORPUS -type f | wc -l | tr -d ' ')  load: $(uptime)"
V=$("$EXIFTOOL_PERL" -I"$ETDIR/lib" "$ETDIR/exiftool" -ver); D=$("$EXIFTOOL_PERL" -I"$ETDIR/lib" "$ETDIR/exiftool" -s3 -FileType "$ETDIR/t/images/OOXML.docx")
echo "oracle -ver=$V docx=$D perl=$EXIFTOOL_PERL"
[ "$V" = 13.59 ] && [ "$D" = DOCX ] || { echo "FATAL oracle probe"; exit 5; }
shasum -a 256 "$CBIN" "$PBIN"; ls -la "$CBIN" "$PBIN"
echo "--- control census $(date +%T)"
(cd "$CTL" && python3 tools/exiftool-tables/conformance.py "$CORPUS" --exiftool-dir "$ETDIR" --oxidex "$CBIN" \
   --recursive --min-files 3875 --min-tags 5000 --json-out "$O/control.json" > "$O/control.txt" 2>&1)
echo "--- control rc=$? $(grep -aE '^TOTAL\b' "$O/control.txt") $(date +%T)"
echo "--- inertness: probe (env unset) vs control binary over every corpus file $(date +%T)"
python3 - "$PBIN" "$CBIN" "$CORPUS" <<'PY'
import json, os, subprocess, sys
probe, ref, corpus = sys.argv[1:4]
drop = {"File:FileAccessDate", "File:FileInodeChangeDate", "FileAccessDate", "FileInodeChangeDate"}
env = {k: v for k, v in os.environ.items() if k != "OXIDEX_PROBE_SILENCE"}
files = sorted(os.path.join(r, f) for r, _, fs in os.walk(corpus) for f in fs)
def run(b, f):
    p = subprocess.run([b, "-j", "-G1", "-a", f], capture_output=True, env=env, timeout=120)
    try:
        d = json.loads(p.stdout or b"[]")
    except Exception:
        return ("RAW", p.returncode, p.stdout)
    for o in d:
        for k in list(o):
            if k in drop or k.endswith(":FileAccessDate") or k.endswith(":FileInodeChangeDate"):
                del o[k]
    return ("JSON", p.returncode, d)
diff = [f for f in files if run(probe, f) != run(ref, f)]
print(f"inertness: {len(files)} files, {len(diff)} differ")
for f in diff[:20]:
    print("  DIFF", f)
PY
for tok in "$@"; do
  name="$(echo "$tok" | tr , +)"
  OXIDEX_PROBE_SILENCE="$tok" "$PBIN" -j "$CORPUS/Apple/Apple_iPhone11.jpg" > /dev/null 2>&1; rc=$?
  echo "--- token $tok sanity rc=$rc $(date +%T)"; [ "$rc" = 0 ] || { echo "SKIP $tok (refused)"; continue; }
  (cd "$PRB" && OXIDEX_PROBE_SILENCE="$tok" python3 tools/exiftool-tables/conformance.py "$CORPUS" --exiftool-dir "$ETDIR" --oxidex "$PBIN" \
     --recursive --min-files 3875 --min-tags 5000 --json-out "$O/probe-$name.json" > "$O/probe-$name.txt" 2>&1)
  echo "--- $tok rc=$? $(grep -aE '^TOTAL\b' "$O/probe-$name.txt") $(date +%T)"
done
echo "CENSUS: DONE $(date +%T)"
