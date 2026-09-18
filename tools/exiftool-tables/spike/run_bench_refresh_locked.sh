#!/usr/bin/env bash
# Regenerate benches/benchmark_results.md under the exclusive measurement lock.
# No absolute load gate (see settle); the comparison script stamps load and the
# top CPU consumers around every scenario.
#   locked.py <log> -- bash tools/exiftool-tables/spike/run_bench_refresh_locked.sh [oxidex-bin]
set -uo pipefail
cd "$(dirname "$0")/../../.." || exit 1
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
echo "=== bench refresh start $(date -u +%FT%TZ) ==="; echo "uptime before: $(uptime)"
bash benches/exiftool_comparison.sh "${1:-target/release/oxidex}"
RC=$?
echo "=== bench refresh exit=$RC ==="; echo "uptime after: $(uptime)"
exit $RC
