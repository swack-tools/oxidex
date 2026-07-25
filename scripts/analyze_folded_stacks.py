#!/usr/bin/env python3
"""
Analyze folded stacks (text format) from inferno/flamegraph profiling.

Folded stacks format:
    function_a;function_b;function_c 100
    function_a;function_d 50

Each line shows a stack trace (semicolon-separated) and sample count.
This script provides accessible text analysis of profiling data.
"""

import sys
from collections import defaultdict, Counter

def parse_folded_stacks(filepath):
    """Parse folded stacks file."""
    stacks = []
    total_samples = 0

    with open(filepath, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue

            # Format: "func_a;func_b;func_c 100"
            parts = line.rsplit(' ', 1)
            if len(parts) != 2:
                continue

            stack_trace = parts[0]
            try:
                count = int(parts[1])
            except ValueError:
                continue

            stacks.append({
                'trace': stack_trace.split(';'),
                'count': count
            })
            total_samples += count

    return stacks, total_samples

def analyze_stacks(stacks, total_samples):
    """Analyze folded stacks and generate report."""

    # Count self time (leaf functions)
    leaf_time = Counter()

    # Count total time (all functions in stack)
    total_time = defaultdict(int)

    for stack in stacks:
        trace = stack['trace']
        count = stack['count']

        # Leaf function (self time)
        if trace:
            leaf_func = trace[-1]
            leaf_time[leaf_func] += count

        # All functions (total time)
        for func in set(trace):  # Use set to count once per stack
            total_time[func] += count

    return leaf_time, total_time

def print_report(leaf_time, total_time, total_samples, top_n=50):
    """Print accessible text report."""

    print("Folded Stacks Analysis")
    print("=" * 80)
    print()
    print(f"Total samples: {total_samples:,}")
    print(f"Unique functions: {len(total_time)}")
    print()

    # Self time (leaf functions - where work happens)
    print("=" * 80)
    print("TOP FUNCTIONS BY SELF TIME (where CPU is actually working)")
    print("=" * 80)
    print()
    print(f"{'Function':<60} {'Samples':<12} {'%':<8}")
    print("-" * 80)

    for func, count in leaf_time.most_common(top_n):
        pct = (count / total_samples * 100) if total_samples > 0 else 0

        # Shorten long function names
        display_func = func if len(func) <= 57 else func[:54] + "..."

        # Highlight oxidex functions
        marker = "***" if ("oxidex" in func.lower() or "::" in func) else "   "

        print(f"{marker} {display_func:<57} {count:<12,} {pct:>6.2f}%")

    # Total time (including callees)
    print()
    print("=" * 80)
    print("TOP FUNCTIONS BY TOTAL TIME (including time in callees)")
    print("=" * 80)
    print()
    print(f"{'Function':<60} {'Samples':<12} {'%':<8}")
    print("-" * 80)

    sorted_total = sorted(total_time.items(), key=lambda x: x[1], reverse=True)
    for func, count in sorted_total[:top_n]:
        pct = (count / total_samples * 100) if total_samples > 0 else 0

        display_func = func if len(func) <= 57 else func[:54] + "..."
        marker = "***" if ("oxidex" in func.lower() or "::" in func) else "   "

        print(f"{marker} {display_func:<57} {count:<12,} {pct:>6.2f}%")

    # OxiDex-specific functions
    oxidex_funcs = [(f, c) for f, c in sorted_total if "oxidex" in f.lower() or "::" in f]
    if oxidex_funcs:
        print()
        print("=" * 80)
        print("OXIDEX-SPECIFIC FUNCTIONS (Rust code)")
        print("=" * 80)
        print()
        print(f"{'Function':<60} {'Samples':<12} {'%':<8}")
        print("-" * 80)

        for func, count in oxidex_funcs[:30]:
            pct = (count / total_samples * 100) if total_samples > 0 else 0
            display_func = func if len(func) <= 60 else func[:57] + "..."
            print(f"{display_func:<60} {count:<12,} {pct:>6.2f}%")

    # Summary statistics
    print()
    print("=" * 80)
    print("SUMMARY")
    print("=" * 80)
    print()

    top_5_self = sum(c for _, c in leaf_time.most_common(5))
    top_5_pct = (top_5_self / total_samples * 100) if total_samples > 0 else 0
    print(f"Top 5 functions (self time): {top_5_pct:.1f}% of total")

    oxidex_total = sum(c for f, c in oxidex_funcs)
    oxidex_pct = (oxidex_total / total_samples * 100) if total_samples > 0 else 0
    print(f"OxiDex functions (total time): {oxidex_pct:.1f}% of total")
    print()

def main():
    if len(sys.argv) < 2:
        print("Usage: analyze_folded_stacks.py <folded_stacks.txt>")
        print()
        print("Analyzes folded stacks (text format) from flamegraph profiling.")
        print("Provides accessible text output with function times and percentages.")
        print()
        print("Generate folded stacks with:")
        print("  dtrace ... | inferno-collapse-dtrace > stacks.txt")
        sys.exit(1)

    filepath = sys.argv[1]

    print(f"Analyzing: {filepath}")
    print()

    stacks, total_samples = parse_folded_stacks(filepath)

    if not stacks:
        print("No data found in file")
        sys.exit(1)

    leaf_time, total_time = analyze_stacks(stacks, total_samples)
    print_report(leaf_time, total_time, total_samples)

if __name__ == '__main__':
    main()
