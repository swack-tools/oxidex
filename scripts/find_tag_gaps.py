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
import shutil
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Fixed, worktree-independent home for durable state (logs, persistent
# worker worktrees) written by the model-fix/tag-fix loops. Deliberately
# NOT REPO_ROOT-relative: these scripts are routinely run from many
# different git worktrees of this repo, and REPO_ROOT-relative paths used
# to scatter a single logical run's logs across whichever worktree
# happened to be cwd when it was launched.
OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))

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


def ensure_tag_comparison_built(repo_root=REPO_ROOT):
    """Build tag-comparison under the "fixloop" profile (see Cargo.toml) --
    this runs on every single round of a fix-loop to re-check gaps, so it's
    a correctness check, not a binary anyone ships; --release's fat LTO and
    single codegen unit make every one of those rebuilds pay a compile cost
    tuned for runtime speed nobody needs here.

    List-argv only, no shell=True anywhere in this file -- repo_root is a
    local path this process already trusts.
    """
    env = dict(os.environ)
    if shutil.which("sccache"):
        # See model_fix_loop.py's cargo_env() -- lets parallel workers
        # (each its own worktree with its own target/) share compiled
        # dependency artifacts instead of every worker cold-compiling the
        # same crates independently.
        env["RUSTC_WRAPPER"] = "sccache"
    subprocess.run(  # nosec B603
        ["cargo", "build", "--profile", "fixloop", "--bin", "tag-comparison", "--features", "tag-comparison-binary"],
        cwd=repo_root, check=True, env=env,
    )


def run_full_comparison(cache_dir, repo_root=REPO_ROOT):
    """Run `just compare-exiftool-full` and return the path to comparison.json."""
    subprocess.run(  # nosec B603
        ["just", "compare-exiftool-full"],
        cwd=repo_root,
        env={**os.environ, "EXIFTOOL_CACHE_DIR": str(cache_dir)},
        check=True,
    )
    return repo_root / "comparison.json"


def run_format_comparison(format_name, cache_dir, repo_root=REPO_ROOT):
    """Re-run tag-comparison for a single format against the cached samples.

    Requires run_full_comparison to have populated cache_dir at least once
    (this does not download or build the combined samples itself).
    """
    ensure_tag_comparison_built(repo_root)
    # Fixed /tmp paths are a race-condition concern on shared multi-user
    # systems; this is a single-developer local CLI tool.
    output = Path(f"/tmp/tagcmp-{format_name}.json")  # nosec B108
    subprocess.run(  # nosec B603 # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit.dangerous-subprocess-use-audit
        [
            str(repo_root / "target/fixloop/tag-comparison"),
            "--exiftool", f"{cache_dir}/exiftool/exiftool",
            "--samples", f"{cache_dir}/combined-samples",
            "--format", format_name,
            "-o", str(output),
            "--markdown-dir", f"/tmp/tagcmp-{format_name}-md",  # nosec B108
        ],
        cwd=repo_root, check=True,
    )
    return output


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="gaps.json")
    parser.add_argument("--only-format")
    # A fixed /tmp default is a race-condition concern on shared multi-user
    # systems; this is a single-developer local CLI tool, and the value is
    # always overridable via EXIFTOOL_CACHE_DIR/--cache-dir.
    parser.add_argument(
        "--cache-dir",
        default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"),  # nosec B108
    )
    args = parser.parse_args(argv)

    if args.only_format:
        report_path = run_format_comparison(args.only_format, args.cache_dir)
    else:
        report_path = run_full_comparison(args.cache_dir)

    report = load_comparison_report(report_path)
    gaps = group_gaps_by_format(report)
    if args.only_format:
        gaps = [g for g in gaps if g["format"] == args.only_format]

    with open(args.output, "w") as f:
        json.dump(gaps, f, indent=2)

    total = sum(g["gap_count"] for g in gaps)
    print(f"{len(gaps)} formats with gaps, {total} total gaps -> {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
