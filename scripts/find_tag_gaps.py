#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Find and group oxidex/ExifTool tag-coverage gaps by format.

Wraps `just compare-exiftool-full` (full corpus) or a direct
`tag-comparison --format` re-run (fast, single-format), then groups the
resulting report's missing_in_oxidex + value_differences by format,
sorted by gap count descending.

Usage:
    uv run scripts/find_tag_gaps.py [--output gaps.json] [--only-format NAME]
                                     [--cache-dir DIR]
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Best-effort format -> source directory/file map, used to hand the model
# real context (it has no file-search tool of its own -- single-shot patch
# generation only). Not authoritative; unlisted formats fall back to a
# lowercase directory guess, and finding nothing is a valid, handled
# outcome (the prompt tells the model these are "likely relevant", not
# exhaustive).
FORMAT_TO_DIR = {
    "JPEG": ["parsers/jpeg", "core"],
    "PNG": ["parsers/png", "core"],
    "TIFF": ["parsers/tiff"],
    "EXIF": ["parsers/tiff"],
    "BMP": ["parsers/image"],
    "GIF": ["parsers/image"],
    "WebP": ["parsers/image"],
    "PDF": ["parsers/pdf"],
    "QuickTime": ["parsers/quicktime"],
    "MP4": ["parsers/quicktime"],
    "MOV": ["parsers/quicktime"],
    "MKV": ["parsers/video"],
    "AVI": ["parsers/video"],
    "RIFF": ["parsers/video"],
    "PE": ["parsers/pe"],
    "ELF": ["parsers/elf"],
    "Mach-O": ["parsers/macho"],
    "ZIP": ["parsers/archive"],
    "DOCX": ["parsers/document"],
    "XLSX": ["parsers/document"],
    "TTF": ["parsers/font"],
    "OTF": ["parsers/font"],
    "DNG": ["parsers/raw"],
    "CR2": ["parsers/raw"],
    "NEF": ["parsers/raw"],
    "ARW": ["parsers/raw"],
    "RAF": ["parsers/raw"],
    "ORF": ["parsers/raw"],
    "RW2": ["parsers/raw"],
    "ICC": ["parsers/icc"],
    "XMP": ["parsers/xmp"],
    "FLAC": ["parsers/audio"],
    "MP3": ["parsers/audio"],
    "AAC": ["parsers/audio"],
    "APE": ["parsers/audio"],
    "Opus": ["parsers/audio"],
    "OGG": ["parsers/audio"],
    "WAV": ["parsers/audio"],
    "FLASHPIX": ["parsers/flashpix"],
    "IPTC": ["parsers/jpeg"],
}


def load_comparison_report(path):
    """Load a tag-comparison ComparisonReport JSON file."""
    with open(path) as f:
        return json.load(f)


def locate_parser_files(format_name, repo_root=REPO_ROOT):
    """Best-effort list of source paths likely responsible for `format_name`.

    Not authoritative -- the model still needs to be told to double-check
    against the actual gap list, but this saves it from starting with
    nothing (it has no file-search tool of its own).
    """
    candidates = FORMAT_TO_DIR.get(format_name, [f"parsers/{format_name.lower()}"])
    found = []
    for rel in candidates:
        path = repo_root / "src" / rel
        if path.is_file():
            found.append(str(path.relative_to(repo_root)))
        elif path.is_dir():
            for rs_file in sorted(path.rglob("*.rs")):
                found.append(str(rs_file.relative_to(repo_root)))
    return found


def group_gaps_by_format(report, repo_root=REPO_ROOT):
    """Group a ComparisonReport's by_format map into a sorted gap list.

    Returns entries only for formats with at least one missing_in_oxidex or
    value_differences entry, sorted by combined gap count descending.
    """
    gaps = []
    for fmt, comp in (report.get("by_format") or {}).items():
        missing = comp.get("missing_in_oxidex") or []
        diffs = comp.get("value_differences") or []
        gap_count = len(missing) + len(diffs)
        if gap_count == 0:
            continue
        gaps.append({
            "format": fmt,
            "missing_tags": missing,
            "value_differences": diffs,
            "gap_count": gap_count,
            "parser_files": locate_parser_files(fmt, repo_root),
        })
    gaps.sort(key=lambda g: g["gap_count"], reverse=True)
    return gaps
