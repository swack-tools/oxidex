#!/usr/bin/env python3
"""
Parse flamegraph SVG files into accessible text format.

Extracts function names and their time percentages from SVG flame graphs,
making the information accessible for screen readers.
"""

import sys
import xml.etree.ElementTree as ET
from collections import defaultdict
import re

def parse_flamegraph_svg(svg_path):
    """Parse flamegraph SVG and extract function timing data."""

    try:
        tree = ET.parse(svg_path)
        root = tree.getroot()
    except Exception as e:
        print(f"Error parsing SVG: {e}")
        return None

    # SVG namespace
    ns = {'svg': 'http://www.w3.org/2000/svg'}

    # Find all <g> elements (flame graph frames)
    frames = []

    for g in root.findall('.//svg:g[@class="func_g"]', ns):
        # Get the <title> element which contains "function_name (samples)"
        title = g.find('svg:title', ns)
        if title is None or not title.text:
            continue

        # Parse title: "function_name (X samples, Y%)"
        title_text = title.text.strip()
        match = re.match(r'(.+?)\s+\(([0-9,]+)\s+samples?,\s+([0-9.]+)%\)', title_text)

        if match:
            func_name = match.group(1)
            samples = match.group(2).replace(',', '')
            percent = float(match.group(3))

            frames.append({
                'function': func_name,
                'samples': int(samples),
                'percent': percent
            })
        else:
            # Try alternate format: "function_name (X samples)"
            match = re.match(r'(.+?)\s+\(([0-9,]+)\s+samples?\)', title_text)
            if match:
                func_name = match.group(1)
                samples = match.group(2).replace(',', '')
                frames.append({
                    'function': func_name,
                    'samples': int(samples),
                    'percent': 0.0  # Will calculate later
                })

    return frames

def aggregate_by_function(frames):
    """Aggregate frames by function name (self time)."""

    func_times = defaultdict(lambda: {'samples': 0, 'percent': 0.0})

    for frame in frames:
        func = frame['function']
        func_times[func]['samples'] += frame['samples']
        func_times[func]['percent'] += frame['percent']

    return func_times

def print_text_report(frames):
    """Print accessible text report of flame graph data."""

    if not frames:
        print("No data found in SVG")
        return

    print("Flamegraph Analysis")
    print("=" * 80)
    print()

    # Aggregate by function
    func_times = aggregate_by_function(frames)

    # Sort by percent (descending)
    sorted_funcs = sorted(
        func_times.items(),
        key=lambda x: x[1]['percent'],
        reverse=True
    )

    # Print header
    print(f"{'Function':<60} {'Samples':<12} {'%':<8}")
    print("-" * 80)

    # Print top functions
    total_samples = sum(f['samples'] for f in frames)

    for func, data in sorted_funcs[:50]:  # Top 50
        # Shorten very long function names
        display_func = func if len(func) <= 57 else func[:54] + "..."

        # Highlight oxidex functions
        marker = "***" if "oxidex" in func or "::" in func.lower() else "   "

        print(f"{marker} {display_func:<57} {data['samples']:<12} {data['percent']:>6.2f}%")

    print()
    print("-" * 80)
    print(f"Total samples: {total_samples}")
    print(f"Unique functions: {len(func_times)}")
    print(f"Showing top 50 functions by time")
    print()

    # Show oxidex-specific functions
    oxidex_funcs = [(f, d) for f, d in sorted_funcs if "oxidex" in f.lower() or "::" in f]
    if oxidex_funcs:
        print()
        print("=" * 80)
        print("OxiDex-Specific Functions (Rust code)")
        print("=" * 80)
        print()
        print(f"{'Function':<60} {'Samples':<12} {'%':<8}")
        print("-" * 80)

        for func, data in oxidex_funcs[:30]:  # Top 30 oxidex functions
            display_func = func if len(func) <= 60 else func[:57] + "..."
            print(f"{display_func:<60} {data['samples']:<12} {data['percent']:>6.2f}%")

def main():
    if len(sys.argv) < 2:
        print("Usage: parse_flamegraph.py <flamegraph.svg>")
        print()
        print("Converts flamegraph SVG to accessible text format.")
        print("Shows functions sorted by time, with oxidex functions highlighted.")
        sys.exit(1)

    svg_path = sys.argv[1]

    print(f"Parsing flamegraph: {svg_path}")
    print()

    frames = parse_flamegraph_svg(svg_path)

    if frames:
        print_text_report(frames)
    else:
        print("Failed to parse flamegraph data")
        sys.exit(1)

if __name__ == '__main__':
    main()
