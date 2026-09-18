#!/usr/bin/env bash
# One lock acquisition for every timed number in the spike:
#   1. run_timing.sh   (hyperfine: oxidex vs pinned ExifTool)
#   2. stages          (in-process stage timer + allocation counts)
#   3. criterion       (dispatch microbench)
# Usage: locked.py <log> -- bash tools/exiftool-tables/spike/run_all_locked.sh <evidence-dir>
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 1
EVID="$1"; OX="$PWD/target/release/oxidex"; ST="$PWD/benches/spike/target/release/stages"
C="${OXIDEX_PINNED_EXIFTOOL:-/tmp/oxidex-exiftool-cache/exiftool}/t/images"
load1() { uptime | sed -E 's/.*load averages?: ([0-9.]+).*/\1/'; }
snap() { echo "[$1] $(uptime)"; ps -Ao pcpu,args -r | head -6 | sed "s/^/[$1]   /" | cut -c1-140; }
bash tools/exiftool-tables/spike/run_timing.sh "$EVID" "$OX" 2>&1 | tee "$EVID/hyperfine.txt"
RC=${PIPESTATUS[0]}; echo "run_timing exit=$RC"; [ "$RC" = 0 ] || exit $RC
echo; echo "=== stages (load1 $(load1)) sha256=$(shasum -a 256 "$ST" | cut -d' ' -f1) ==="; snap "stages before"
"$ST" --reps 300 "$C/Canon.jpg" "$C/Nikon.nef" 2>/dev/null | tee "$EVID/stages-two-files.tsv"
"$ST" --reps 30 "$C"/* 2>/dev/null > "$EVID/stages-corpus.tsv"; tail -1 "$EVID/stages-corpus.tsv"; snap "stages after"
echo; echo "=== criterion dispatch (load1 $(load1)) ==="; snap "criterion before"
( cd benches/spike && cargo bench --bench dispatch -- --noplot --warm-up-time 1 --measurement-time 2 2>&1 ) | tee "$EVID/criterion.txt" | rg 'time:|thrpt:|^[a-z_]+/' 
snap "criterion after"
echo; echo "=== samply re-record under lock (load1 $(load1)) ==="
samply record --rate 4000 --save-only --unstable-presymbolicate -o "$EVID/samply-stages-corpus-locked.json.gz" -- "$ST" --reps 100 "$C"/* > /dev/null 2>"$EVID/samply-stages-corpus-locked.err"
python3 tools/exiftool-tables/spike/samply_buckets.py "$EVID/samply-stages-corpus-locked.json.gz" --top 60 > "$EVID/samply-stages-corpus-locked.buckets.txt" 2>&1; sed -n 1,40p "$EVID/samply-stages-corpus-locked.buckets.txt"
samply record --rate 4000 --iteration-count 40 --save-only --unstable-presymbolicate -o "$EVID/samply-cli-canon-locked.json.gz" -- "$OX" -j -a -G1 "$C/Canon.jpg" > /dev/null 2>"$EVID/samply-cli-canon-locked.err"
python3 tools/exiftool-tables/spike/samply_buckets.py "$EVID/samply-cli-canon-locked.json.gz" --top 40 > "$EVID/samply-cli-canon-locked.buckets.txt" 2>&1; sed -n 1,40p "$EVID/samply-cli-canon-locked.buckets.txt"
samply record --rate 4000 --iteration-count 40 --save-only --unstable-presymbolicate -o "$EVID/samply-cli-nef-locked.json.gz" -- "$OX" -j -a -G1 "$C/Nikon.nef" > /dev/null 2>"$EVID/samply-cli-nef-locked.err"
python3 tools/exiftool-tables/spike/samply_buckets.py "$EVID/samply-cli-nef-locked.json.gz" --top 40 > "$EVID/samply-cli-nef-locked.buckets.txt" 2>&1; sed -n 1,40p "$EVID/samply-cli-nef-locked.buckets.txt"
snap "profiles after"
if [ "${OXIDEX_BENCH_REFRESH:-0}" = 1 ]; then echo; echo "=== bench refresh (same exclusive hold) ==="; bash tools/exiftool-tables/spike/run_bench_refresh_locked.sh "$OX"; fi
echo "ALL DONE load1 $(load1)"
