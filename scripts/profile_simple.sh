#!/bin/bash
# Simple text-based profiling using Criterion's built-in measurement

echo "Running benchmarks with detailed timing..."
echo "=========================================="
echo

# Run each benchmark individually to see detailed breakdown
for benchmark in format_detection jpeg_segment_parsing tiff_ifd_parsing full_read_metadata iptc_tag_name_generation; do
    echo "Benchmark: $benchmark"
    echo "---"
    cargo bench --bench parse_benchmarks $benchmark 2>&1 | grep -A 3 "time:"
    echo
done

echo "=========================================="
echo "Integration benchmarks:"
echo

cargo bench --bench integration_benchmarks 2>&1 | grep -E "(Benchmarking|time:)" | head -50

echo
echo "=========================================="
echo "Summary: Look for benchmarks with highest times (ms)"
echo "Those are the optimization targets."
