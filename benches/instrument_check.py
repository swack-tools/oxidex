#!/usr/bin/env python3
"""Instrument preflight for benches/exiftool_comparison.sh.

Refuses to let the comparison run against a binary or an oracle it cannot
name (AGENTS.md incidents #1 "implicit binary resolution" and #2 "a stale
prebuilt binary", and the "a matching -ver is not a working oracle" rule):

  * the oxidex binary is resolved through scripts/instrument.py's
    resolve_binary() -- exits loudly if the path is not a file -- and its
    staleness_note() is printed (and exported) so a binary older than HEAD or
    a dirty file cannot be graded silently;
  * the ExifTool oracle comes from scripts/exiftool_oracle.py, must report the
    release in .exiftool-version AND pass the OOXML.docx capability probe,
    otherwise exit 2;
  * the load averages (1/5/15 min) and the top CPU consumers are recorded;
    with --max-load N the run is refused above N unless OXIDEX_ALLOW_LOAD=1
    (no gate by default: the host this runs on idles at load1 4-6 from
    processes outside our control, so the number is stamped, not gated).

Prints the standard `=== instrument: ... ===` header to stderr and
KEY=VALUE lines to stdout for the shell script to `eval`.

Usage: instrument_check.py <oxidex-binary> [--max-load N]
"""
from __future__ import annotations

import hashlib
import os
import platform
import shlex
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import exiftool_oracle  # noqa: E402
import instrument  # noqa: E402


def loadavg() -> tuple[float, float, float] | None:
    try:
        return os.getloadavg()
    except OSError:
        return None


def top_cpu(n: int = 5) -> list[str]:
    # BSD/macOS ps sorts by CPU with -r; procps (Linux) has no -r and uses --sort.
    sort = ["-r"] if sys.platform == "darwin" else ["--sort=-pcpu"]
    out = subprocess.run(["ps", "-Ao", "pcpu,comm", *sort], capture_output=True, text=True).stdout.splitlines()
    return [line.strip()[:100] for line in out[1 : n + 1]]


def main() -> int:
    args = sys.argv[1:]
    max_load = None
    if "--max-load" in args:
        i = args.index("--max-load")
        max_load = float(args[i + 1])
        del args[i : i + 2]
    if len(args) != 1:
        sys.exit(__doc__)
    binary = instrument.resolve_binary(args[0], kind="oxidex")  # exits loudly if absent
    git = instrument.git_state(ROOT)
    note = instrument.staleness_note(binary, git)
    sha = hashlib.sha256(binary.path.read_bytes()).hexdigest()
    version = subprocess.run([str(binary.path), "--version"], capture_output=True, text=True).stdout.strip()

    if os.environ.get("OXIDEX_BENCHMARK_CI") == "1":
        for name in ("PERL5LIB", "PERLLIB", "PERL5OPT", "EXIFTOOL"):
            os.environ.pop(name, None)
        os.environ["EXIFTOOL_HOME"] = os.devnull
        try:
            oracle = exiftool_oracle.resolve_ci_tree(
                os.environ["EXIFTOOL_SOURCE"], os.environ["OXIDEX_TABLES_PERL"]
            )
        except (KeyError, exiftool_oracle.OracleError) as exc:
            print(f"❌ CI oracle refused: {exc}", file=sys.stderr)
            return 2
        corpus = Path(os.environ["EXIFTOOL_SOURCE"]).resolve() / "t/images"
        docx = corpus / "OOXML.docx"
    else:
        oracle = exiftool_oracle.resolve_or_exit()
        corpus = Path(oracle.argv[-1]).parent / "t/images"
        docx = exiftool_oracle.cache_dir() / "combined-samples" / "OOXML.docx"
    pin = exiftool_oracle.repo_pin()
    if not docx.is_file():
        docx = corpus / "OOXML.docx"
    problems = []
    if not pin:
        problems.append("no .exiftool-version pin found")
    elif oracle.version != pin:
        problems.append(f"oracle is ExifTool {oracle.version}, .exiftool-version pins {pin}")
    if oracle.missing_modules:
        problems.append(f"oracle perl is missing {', '.join(oracle.missing_modules)}")
    try:
        oracle.check_container_support(docx)
        docx_probe = "DOCX"
    except exiftool_oracle.OracleError as exc:
        docx_probe = "FAILED"
        problems.append(str(exc))

    loads = loadavg()
    load = loads[0] if loads else None
    top = top_cpu()
    machine = f"{platform.system()} {platform.release()} {platform.machine()}"
    if sys.platform == "darwin":
        cpu = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip()
        ncpu = subprocess.run(["sysctl", "-n", "hw.ncpu"], capture_output=True, text=True).stdout.strip()
    else:
        cpu = platform.processor() or "unknown"
        ncpu = str(os.cpu_count())

    old_stdout = sys.stdout
    sys.stdout = sys.stderr
    try:
        instrument.print_header(
            tool="benches/exiftool_comparison.sh",
            git=git,
            binary=binary,
            oracle=oracle,
            extra=[
                f"         sha256 {sha}",
                f"         {version}",
                f"docx:    {docx_probe} ({docx})",
                f"machine: {machine}, {cpu}, {ncpu} cores",
                f"load:    {' '.join(f'{x:.2f}' for x in loads) if loads else 'unknown'} (1/5/15 min)"
                + (f", gate {max_load}" if max_load is not None else ", no gate"),
                "top CPU: " + "; ".join(top),
            ],
        )
    finally:
        sys.stdout = old_stdout

    if git.dirty and os.environ.get("OXIDEX_ALLOW_DIRTY_TREE") != "1":
        print("❌ working tree is dirty; the numbers could not be attributed to a commit. "
              "Commit, stash, or set OXIDEX_ALLOW_DIRTY_TREE=1.", file=sys.stderr)
        return 2
    for p in problems:
        print(f"❌ {p}", file=sys.stderr)
    if problems:
        print("❌ refusing to benchmark against an oracle that is not the pinned, working ExifTool", file=sys.stderr)
        return 2
    if max_load is not None and load is not None and load > max_load and os.environ.get("OXIDEX_ALLOW_LOAD") != "1":
        print(f"❌ load1 {load:.2f} > {max_load}: timing on a contended box is not a measurement. "
              "Wait, or set OXIDEX_ALLOW_LOAD=1 to record it anyway.", file=sys.stderr)
        return 3

    kv = {
        "OXIDEX_BIN": str(binary.path),
        "OXIDEX_SHA256": sha,
        "OXIDEX_VERSION": version,
        "OXIDEX_STALENESS": note or "",
        "EXIFTOOL_CMD": shlex.join(oracle.argv),
        "EXIFTOOL_VERSION": oracle.version,
        "EXIFTOOL_PROVENANCE": oracle.provenance(),
        "EXIFTOOL_CORPUS": str(corpus),
        "GIT_COMMIT": git.commit or "unknown",
        "GIT_DESCRIBE": git.describe or "",
        "GIT_DIRTY": "1" if git.dirty else "0",
        "LOAD1": f"{load:.2f}" if load is not None else "unknown",
        "LOADAVG": " ".join(f"{x:.2f}" for x in loads) if loads else "unknown",
        "TOP_CPU": "; ".join(top),
        "MACHINE": f"{machine}; {cpu}; {ncpu} cores",
    }
    for k, v in kv.items():
        print(f"{k}={shlex.quote(v)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
