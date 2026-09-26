#!/usr/bin/env bash
# Synthetic CPU, memory and disk benchmarks, identical on every runner. Writes
# one JSON file that summarize.py compares across runners.
#
#   bash synthetic.sh <runner-name> <run-number> <out.json>
#
# BENCH_QUICK=1 shortens every test, for checking the script rather than
# measuring anything. Needs sysbench, fio and jq.
set -euo pipefail

name=$1 run=$2
out=$(realpath -m "$3")
mkdir -p "$(dirname "$out")"
t_cpu=15 t_mem=10 t_disk=20 disk_size=2G
if [ -n "${BENCH_QUICK:-}" ]; then t_cpu=1 t_mem=1 t_disk=2 disk_size=64M; fi

# CPU: sysbench prime sieve, events/s. Thread counts run from one up to the
# self-hosted box's 12; GitHub's runner has 4 vCPUs.
cpu() {
  sysbench cpu --cpu-max-prime=20000 --threads="$1" --time="$t_cpu" run |
    awk '/events per second/ {print $NF}'
}

# Memory bandwidth, MiB/s. The total is set high so --time ends each test.
mem() {
  sysbench memory --memory-block-size=1M --memory-total-size=1000G \
    --memory-oper="$2" --threads="$1" --time="$t_mem" run |
    awk '/MiB transferred/ {gsub(/[()]/, ""); print $4}'
}

# Disk: fio in the runner's temp dir, the filesystem builds use (overlayfs on
# NVMe for self-hosted). O_DIRECT bypasses the page cache; if the filesystem
# refuses it, fall back to buffered I/O and record that, since buffered reads
# largely measure RAM.
fio_dir=${RUNNER_TEMP:-/tmp}/fio
mkdir -p "$fio_dir"
disk_mode=direct
fio_run() { # <test> <rw> <block size> <iodepth>
  local args=(--name="$1" --filename="$fio_dir/fio.dat" --rw="$2" --bs="$3"
    --iodepth="$4" --size="$disk_size" --ioengine=libaio --runtime="$t_disk"
    --time_based --output-format=json --output="$fio_dir/$1.json")
  if [ "$disk_mode" = direct ] && fio --direct=1 "${args[@]}" >/dev/null 2>&1; then
    return
  fi
  disk_mode=buffered
  fio --direct=0 "${args[@]}" >/dev/null
}
fio_get() { # <test> <read|write> <bw|iops>
  jq -r ".jobs[0].$2.$3" "$fio_dir/$1.json" 2>/dev/null || true
}

cpu_1t=$(cpu 1) cpu_2t=$(cpu 2) cpu_3t=$(cpu 3) cpu_4t=$(cpu 4)
cpu_6t=$(cpu 6) cpu_8t=$(cpu 8) cpu_12t=$(cpu 12)
mem_read_1t=$(mem 1 read) mem_write_1t=$(mem 1 write)
mem_read_4t=$(mem 4 read) mem_write_4t=$(mem 4 write)
fio_run seqwrite write 1M 16
fio_run seqread read 1M 16
fio_run randwrite randwrite 4k 32
fio_run randread randread 4k 32
rm -f "$fio_dir/fio.dat"

jq -n \
  --arg runner "$name" --arg run "$run" --arg disk_mode "$disk_mode" \
  --arg os "$(sed -n 's/^PRETTY_NAME="\(.*\)"$/\1/p' /etc/os-release)" \
  --arg cpu_model "$(lscpu | sed -n 's/^Model name:[[:space:]]*//p' | head -1)" \
  --arg nproc "$(nproc)" \
  --arg cpu_1t "$cpu_1t" --arg cpu_2t "$cpu_2t" --arg cpu_3t "$cpu_3t" \
  --arg cpu_4t "$cpu_4t" --arg cpu_6t "$cpu_6t" --arg cpu_8t "$cpu_8t" \
  --arg cpu_12t "$cpu_12t" \
  --arg mem_read_1t "$mem_read_1t" --arg mem_write_1t "$mem_write_1t" \
  --arg mem_read_4t "$mem_read_4t" --arg mem_write_4t "$mem_write_4t" \
  --arg seq_read_kib "$(fio_get seqread read bw)" \
  --arg seq_write_kib "$(fio_get seqwrite write bw)" \
  --arg rand_read_iops "$(fio_get randread read iops)" \
  --arg rand_write_iops "$(fio_get randwrite write iops)" \
  'def n: tonumber? // null;
   def mib: n | if . then . / 1024 else null end;
   {runner: $runner, run: ($run | n), os: $os, cpu_model: $cpu_model,
    nproc: ($nproc | n), disk_mode: $disk_mode,
    cpu_1t: ($cpu_1t | n), cpu_2t: ($cpu_2t | n), cpu_3t: ($cpu_3t | n),
    cpu_4t: ($cpu_4t | n), cpu_6t: ($cpu_6t | n), cpu_8t: ($cpu_8t | n),
    cpu_12t: ($cpu_12t | n),
    mem_read_1t: ($mem_read_1t | n), mem_write_1t: ($mem_write_1t | n),
    mem_read_4t: ($mem_read_4t | n), mem_write_4t: ($mem_write_4t | n),
    disk_seq_read: ($seq_read_kib | mib), disk_seq_write: ($seq_write_kib | mib),
    disk_rand_read: ($rand_read_iops | n), disk_rand_write: ($rand_write_iops | n)}' \
  >"$out"
cat "$out"
