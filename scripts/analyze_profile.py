#!/usr/bin/env python3
"""Analyze samply/Firefox Profiler JSON to extract hotspots."""

import json
import sys
from collections import Counter, defaultdict

def analyze_profile(profile_path):
    print(f"Loading profile from {profile_path}...")
    with open(profile_path, 'r') as f:
        profile = json.load(f)

    # Firefox Profiler format has threads array
    threads = profile.get('threads', [])

    if not threads:
        print("No threads found in profile!")
        return

    print(f"\nFound {len(threads)} thread(s)")

    # Analyze each thread
    for thread_idx, thread in enumerate(threads):
        thread_name = thread.get('name', f'Thread {thread_idx}')
        print(f"\n{'='*80}")
        print(f"Thread: {thread_name}")
        print(f"{'='*80}")

        # Get samples and stacks
        samples = thread.get('samples', {})
        stack_table = thread.get('stackTable', {})
        frame_table = thread.get('frameTable', {})
        func_table = thread.get('funcTable', {})
        string_table = thread.get('stringArray', [])

        if not samples:
            print("No samples in this thread")
            continue

        # Count samples per stack
        stack_ids = samples.get('stack', [])
        weights = samples.get('weight', [1] * len(stack_ids))

        print(f"Total samples: {sum(weights)}")

        # Build function name lookup
        def get_string(idx):
            if idx is None or idx >= len(string_table):
                return "<unknown>"
            return string_table[idx]

        def get_func_name(func_idx):
            if func_idx is None or func_idx >= len(func_table.get('name', [])):
                return "<unknown>"
            name_idx = func_table['name'][func_idx]
            return get_string(name_idx)

        def get_frame_func(frame_idx):
            if frame_idx is None or frame_idx >= len(frame_table.get('func', [])):
                return "<unknown>"
            func_idx = frame_table['func'][frame_idx]
            return get_func_name(func_idx)

        # Count samples per function (self time)
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
            while current_stack is not None and current_stack < len(stack_frame):
                frame_idx = stack_frame[current_stack]
                func_name = get_frame_func(frame_idx)
                if func_name not in seen_funcs:
                    func_total_samples[func_name] += weight
                    seen_funcs.add(func_name)

                # Move to parent stack
                if current_stack < len(stack_prefix):
                    current_stack = stack_prefix[current_stack]
                else:
                    break

        # Print top functions by self time
        print(f"\n{'─'*80}")
        print("TOP FUNCTIONS BY SELF TIME (where actual work happens):")
        print(f"{'─'*80}")
        print(f"{'Samples':<10} {'%':<8} {'Function'}")
        print(f"{'─'*80}")

        total_samples = sum(weights)
        for func, count in func_samples.most_common(30):
            pct = (count / total_samples * 100) if total_samples > 0 else 0
            print(f"{count:<10} {pct:>6.2f}%  {func}")

        # Print top functions by total time
        print(f"\n{'─'*80}")
        print("TOP FUNCTIONS BY TOTAL TIME (including callees):")
        print(f"{'─'*80}")
        print(f"{'Samples':<10} {'%':<8} {'Function'}")
        print(f"{'─'*80}")

        for func, count in func_total_samples.most_common(30):
            pct = (count / total_samples * 100) if total_samples > 0 else 0
            print(f"{count:<10} {pct:>6.2f}%  {func}")

if __name__ == '__main__':
    if len(sys.argv) < 2:
        print("Usage: analyze_profile.py <profile.json>")
        sys.exit(1)

    analyze_profile(sys.argv[1])
