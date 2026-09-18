#!/usr/bin/env bash
# Spike timing harness. Run ONLY under the shared measurement lock:
#   locked.py <log> -- bash tools/exiftool-tables/spike/run_timing.sh <evidence-dir> <oxidex-bin>
# Records load1/5/15 and the top CPU consumers around every timed run (no absolute load gate: this host idles at 4-6).
set -euo pipefail
EVID="$1"; OXIDEX="$2"
ET="${OXIDEX_PINNED_EXIFTOOL:-/tmp/oxidex-exiftool-cache/exiftool}/exiftool"
CORPUS="$(dirname "$ET")/t/images"
unset PERL5LIB PERLLIB PERL5OPT
export PATH=/tmp/oxidex-perl538-build-20260913-r2/prefix/bin:$PATH
mkdir -p "$EVID"
echo "=== instrument: run_timing.sh ==="
echo "date: $(date -u +%FT%TZ)"
echo "commit: $(git rev-parse HEAD) dirty=$(git status --porcelain | wc -l | tr -d ' ')"
echo "oxidex: $OXIDEX sha256=$(shasum -a 256 "$OXIDEX" | cut -d' ' -f1) mtime=$(stat -f %Sm "$OXIDEX")"
echo "oxidex --version: $("$OXIDEX" --version)"
echo "exiftool: $ET -ver=$("$ET" -ver) perl=$(which perl) docx-probe=$("$ET" -s3 -FileType "$CORPUS/OOXML.docx")"
[ "$("$ET" -ver)" = "13.59" ] || { echo "REFUSE: exiftool -ver != 13.59"; exit 2; }
[ "$("$ET" -s3 -FileType "$CORPUS/OOXML.docx")" = "DOCX" ] || { echo "REFUSE: docx probe failed"; exit 2; }
echo "corpus: $CORPUS files=$(ls "$CORPUS" | wc -l | tr -d ' ')"
echo "hyperfine: $(hyperfine --version)"
load1() { uptime | sed -E 's/.*load averages?: ([0-9.]+).*/\1/'; }
# No absolute load gate: this host idles at load1 4-6 (CrashPlanService and
# Mail run permanently). Wait at most 60 s for load1 to stop falling, then go.
settle() {
  local prev cur i
  prev=$(load1)
  for i in 1 2 3 4 5 6; do
    sleep 10; cur=$(load1)
    if (( $(echo "$cur >= $prev" | bc -l) )); then break; fi
    prev=$cur
  done
  echo "settled after $((i*10))s at load1 $cur"
}
# Machine state around a timed run: load1/5/15 and the top 5 CPU consumers.
snap() {
  echo "[$1] $(uptime)"
  ps -Ao pcpu,args -r | head -6 | sed "s/^/[$1]   /" | cut -c1-140
}
settle
LOAD=$(load1)
echo "load1: $LOAD"
echo "nproc: $(sysctl -n hw.ncpu)"
FILES=$(ls "$CORPUS"/* | tr '\n' ' ')
run() { # name, then commands
  local name="$1"; shift
  echo; echo "--- $name (load1 before: $(load1))"
  snap "$name before"
  hyperfine --warmup 5 --runs 30 -N --export-json "$EVID/$name.json" --export-markdown "$EVID/$name.md" "$@"
  snap "$name after"
}
run canon_jpg "$OXIDEX -j -a -G1 $CORPUS/Canon.jpg" "$ET -j -a -G1 $CORPUS/Canon.jpg"
run nikon_nef "$OXIDEX -j -a -G1 $CORPUS/Nikon.nef" "$ET -j -a -G1 $CORPUS/Nikon.nef"
run corpus_parallel "$OXIDEX -j -a -G1 $FILES" "$ET -j -a -G1 $FILES"
echo; echo "--- corpus_1thread (RAYON_NUM_THREADS=1 for oxidex; load1: $(load1))"
snap "corpus_1thread before"
RAYON_NUM_THREADS=1 hyperfine --warmup 5 --runs 30 -N --export-json "$EVID/corpus_1thread.json" --export-markdown "$EVID/corpus_1thread.md" "$OXIDEX -j -a -G1 $FILES" "$ET -j -a -G1 $FILES"
snap "corpus_1thread after"
echo; echo "--- oxidex_noop (process startup floor: --version)"
snap "noop before"
hyperfine --warmup 5 --runs 30 -N --export-json "$EVID/noop.json" "$OXIDEX --version" "$ET -ver"
snap "noop after"
