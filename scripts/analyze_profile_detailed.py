#!/usr/bin/env python3
"""Analyze samply/Firefox Profiler JSON - focus on high-sample threads."""

import json
import sys
from collections import Counter

def analyze_profile(profile_path, min_samples=1000):
    print(f"Loading profile from {profile_path}...")
    with open(profile_path, 'r') as f:
        profile = json.load(f)

    threads = profile.get('threads', [])
    print(f"\nFound {len(threads)} thread(s)")

    # Analyze only threads with significant samples
    for thread_idx, thread in enumerate(threads):
        thread_name = thread.get('name', f'Thread {thread_idx}')

        samples = thread.get('samples', {})
        stack_ids = samples.get('stack', [])
        weights = samples.get('weight', [1] * len(stack_ids))
        total_samples = sum(weights)

        if total_samples < min_samples:
            continue

        print(f"\n{'='*80}")
        print(f"Thread: {thread_name} ({total_samples} samples)")
        print(f"{'='*80}")

        stack_table = thread.get('stackTable', {})
        frame_table = thread.get('frameTable', {})
        func_table = thread.get('funcTable', {})
        string_table = thread.get('stringArray', [])

        # Build lookups
        def get_string(idx):
            if idx is None or idx >= len(string_table):
                return f"<unknown:{idx}>"
            return string_table[idx]

        def get_func_name(func_idx):
            if func_idx is None or func_idx >= len(func_table.get('name', [])):
                return f"<unknown func:{func_idx}>"
            name_idx = func_table['name'][func_idx]
            name = get_string(name_idx)
            # Also try to get file info
            file_idx = func_table.get('fileName', [None] * (func_idx + 1))[func_idx] if func_idx < len(func_table.get('fileName', [])) else None
            if file_idx is not None:
                file_name = get_string(file_idx)
                if file_name and 'oxidex' in file_name:
                    return f"{name} [{file_name}]"
            return name

        def get_frame_func(frame_idx):
            if frame_idx is None or frame_idx >= len(frame_table.get('func', [])):
                return f"<unknown frame:{frame_idx}>"
            func_idx = frame_table['func'][frame_idx]
            return get_func_name(func_idx)

        # Count samples per function
        func_samples = Counter()
        func_total_samples = Counter()

        stack_frame = stack_table.get('frame', [])
        stack_prefix = stack_table.get('prefix', [])

        for stack_id, weight in zip(stack_ids, weights):
            if stack_id is None:
                continue

            # Get leaf frame (self time)
            if stack_id < len(stack_frame):
                frame_idx = stack_frame[stack_id]
                func_name = get_frame_func(frame_idx)
                func_samples[func_name] += weight

            # Walk up stack for total time
            current_stack = stack_id
            seen_funcs = set()
            depth = 0
            while current_stack is not None and current_stack < len(stack_frame) and depth < 100:
                frame_idx = stack_frame[current_stack]
                func_name = get_frame_func(frame_idx)
                if func_name not in seen_funcs:
                    func_total_samples[func_name] += weight
                    seen_funcs.add(func_name)

                if current_stack < len(stack_prefix):
                    current_stack = stack_prefix[current_stack]
                else:
                    break
                depth += 1

        # Print top functions by self time
        print(f"\n{'─'*80}")
        print("TOP 40 FUNCTIONS BY SELF TIME (where CPU is actually working):")
        print(f"{'─'*80}")
        print(f"{'Samples':<10} {'%':<8} {'Function'}")
        print(f"{'─'*80}")

        for func, count in func_samples.most_common(40):
            pct = (count / total_samples * 100) if total_samples > 0 else 0
            # Highlight oxidex functions
            marker = "***" if "oxidex" in func or "::" in func else "   "
            print(f"{count:<10} {pct:>6.2f}%  {marker} {func}")

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: analyze_profile_detailed.py <profile.json>")
        sys.exit(1)

    min_samples = int(sys.argv[2]) if len(sys.argv) > 2 else 1000
    analyze_profile(sys.argv[1], min_samples)
