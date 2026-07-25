#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Spec section 5 / Phase 5 rollout: scale-gate checks before raising
max_parallel toward the 50-lane step.

This is a READ-ONLY REPORTING TOOL, not an enforcement mechanism (spec
section 5: "nothing in this spec depends on"[automatic scaling] -- "a
human reads this and decides"). Nothing here writes config.toml, touches
max_parallel, or changes any running process; it only inspects existing
state (config.toml, rate-governor.json, manifest.log + tags-found.log via
watch_parallel_fix.py's own tier KPI helpers, and disk headroom on the
OXIDEX_HOME filesystem) and prints a PASS/FAIL per gate plus one overall
verdict.

The three gates (spec section 5, "Gate to enable [the 50-lane step]"):
  1. governor_calls_per_minute >= 60 granted (configured in config.toml;
     rate-governor.json's consecutive_limited is reported alongside as a
     corroborating "is the account actually sustaining this" signal, but
     the pass/fail line itself is about the CONFIGURED ceiling, since
     that's the knob an operator is about to raise).
  2. measured T3 calls-per-landed-tag < 10 (scripts/watch_parallel_fix.py's
     tier_kpi_stats, spec section 5's KPI, read from manifest.log +
     tags-found.log).
  3. >= 100 GB disk headroom on the OXIDEX_HOME filesystem
     (shutil.disk_usage).

Usage:
    uv run scripts/check_scale_gate.py
    uv run scripts/check_scale_gate.py --home ~/.oxidex --config config.toml
"""
import argparse
import json
import os
import shutil
import sys
import tomllib
from pathlib import Path

from watch_parallel_fix import parse_manifest_log_tiered, parse_tags_found_log_tiered, tier_kpi_stats

REPO_ROOT = Path(__file__).resolve().parent.parent
OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))

DEFAULT_CONFIG_PATH = REPO_ROOT / "config.toml"
DEFAULT_GOVERNOR_PATH = OXIDEX_HOME / "logs" / "rate-governor.json"
DEFAULT_MANIFEST_PATH = OXIDEX_HOME / "logs" / "model-fix-requests" / "manifest.log"
DEFAULT_TAGS_FOUND_LOG_PATH = OXIDEX_HOME / "logs" / "tags-found.log"

# Spec section 5's own numbers for the 20 -> 50 lane step.
REQUIRED_GOVERNOR_CALLS_PER_MINUTE = 60
REQUIRED_T3_CALLS_PER_TAG = 10
REQUIRED_DISK_HEADROOM_GB = 100

GB = 1024 ** 3


def read_configured_governor_rate(config_path):
    """[worker].governor_calls_per_minute from config.toml, or None if the
    file/table/key is missing or unparseable -- a caller-visible "can't
    tell" rather than a silently-defaulted number that would make an
    ungated/misconfigured host look artificially fine."""
    try:
        with open(config_path, "rb") as f:
            data = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError):
        return None
    worker = data.get("worker")
    if not isinstance(worker, dict):
        return None
    return worker.get("governor_calls_per_minute")


def read_governor_consecutive_limited(governor_path):
    """rate-governor.json's own consecutive_limited counter (corroborating
    signal, not the gate itself -- see module docstring), or None if the
    file is missing/unparseable."""
    try:
        data = json.loads(Path(governor_path).read_text())
    except (OSError, json.JSONDecodeError):
        return None
    if not isinstance(data, dict):
        return None
    return data.get("consecutive_limited")


def governor_gate(config_path, governor_path, required=REQUIRED_GOVERNOR_CALLS_PER_MINUTE):
    """{"name", "passed", "measured", "required", "detail"} for the
    governor-rate gate. measured is the CONFIGURED governor_calls_per_minute
    (None if unreadable -- FAILS closed, since an operator raising
    max_parallel without knowing the configured rate is exactly the
    unsafe case this gate exists to catch)."""
    configured = read_configured_governor_rate(config_path)
    consecutive_limited = read_governor_consecutive_limited(governor_path)
    passed = configured is not None and configured >= required
    detail = f"configured governor_calls_per_minute={configured!r}"
    if consecutive_limited is not None:
        detail += f", rate-governor.json consecutive_limited={consecutive_limited}"
        if consecutive_limited > 0:
            detail += " (recent 429/5xx cooldowns -- the account may not actually sustain the configured rate)"
    return {
        "name": "governor_calls_per_minute", "passed": passed,
        "measured": configured, "required": required, "detail": detail,
    }


def t3_calls_per_tag_gate(manifest_path, tags_found_log_path, required=REQUIRED_T3_CALLS_PER_TAG):
    """{"name", "passed", "measured", "required", "detail"} for the T3
    calls-per-landed-tag gate, via watch_parallel_fix.py's own tier KPI
    helpers (spec section 5's KPI -- reused rather than reimplemented, so
    this tool and the live dashboard can never silently disagree on the
    number). No T3 data at all (no table-port has landed yet) FAILS
    closed with an explicit "no data yet" detail -- distinct from a
    measured-and-too-high failure, but still not a pass: "under 10
    measured" cannot be claimed without a measurement."""
    manifest_entries = parse_manifest_log_tiered(manifest_path)
    found_entries = parse_tags_found_log_tiered(tags_found_log_path)
    kpi = tier_kpi_stats(manifest_entries, found_entries)
    t3 = kpi.get("T3")
    if t3 is None or t3["landed"] == 0:
        return {
            "name": "t3_calls_per_landed_tag", "passed": False, "measured": None, "required": required,
            "detail": "no T3 table-port has landed yet -- nothing measured",
        }
    measured = t3["calls_per_landed_tag"]
    passed = measured is not None and measured < required
    return {
        "name": "t3_calls_per_landed_tag", "passed": passed, "measured": measured, "required": required,
        "detail": f"{t3['calls']} calls / {t3['landed']} landed tags = {measured:.1f} calls/tag",
    }


def disk_headroom_gate(path, required_gb=REQUIRED_DISK_HEADROOM_GB):
    """{"name", "passed", "measured", "required", "detail"} for the disk-
    headroom gate -- shutil.disk_usage on the filesystem `path` lives on
    (spec section 5: "160-something GB free" was the whole reason the
    100-worker scale was ruled out on this host; this is the same check,
    runnable on demand instead of eyeballed once). measured is in GB
    (binary, 1024**3), rounded to 1 decimal for display."""
    try:
        usage = shutil.disk_usage(path)
    except OSError as e:
        return {
            "name": "disk_headroom_gb", "passed": False, "measured": None, "required": required_gb,
            "detail": f"could not stat {path}: {e}",
        }
    free_gb = usage.free / GB
    passed = free_gb >= required_gb
    return {
        "name": "disk_headroom_gb", "passed": passed, "measured": round(free_gb, 1), "required": required_gb,
        "detail": f"{free_gb:.1f} GB free on the filesystem holding {path}",
    }


def run_all_gates(config_path=DEFAULT_CONFIG_PATH, governor_path=DEFAULT_GOVERNOR_PATH,
                  manifest_path=DEFAULT_MANIFEST_PATH, tags_found_log_path=DEFAULT_TAGS_FOUND_LOG_PATH,
                  disk_path=OXIDEX_HOME):
    """Every gate's result plus an overall verdict -- PASS only if every
    gate passes (spec section 5: "50 lanes only when" ALL of these hold).
    Pure aggregation of the three gate functions above; no filesystem
    access of its own beyond what it delegates to them."""
    gates = [
        governor_gate(config_path, governor_path),
        t3_calls_per_tag_gate(manifest_path, tags_found_log_path),
        disk_headroom_gate(disk_path),
    ]
    overall_passed = all(g["passed"] for g in gates)
    return {"gates": gates, "overall_passed": overall_passed}


def format_report(result):
    lines = ["Scale-gate check (spec section 5 -- the 20 -> 50 lane step)", "=" * 60]
    for gate in result["gates"]:
        status = "PASS" if gate["passed"] else "FAIL"
        lines.append(f"[{status}] {gate['name']}: {gate['detail']}")
    lines.append("=" * 60)
    lines.append(f"OVERALL: {'PASS' if result['overall_passed'] else 'FAIL'}"
                + ("" if result["overall_passed"] else
                   " -- do not raise max_parallel toward the 50-lane step until every gate passes"))
    lines.append("")
    lines.append(
        "This tool is READ-ONLY reporting -- it never changes max_parallel or any other "
        "config. A human reads this and decides (spec section 5)."
    )
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", default=str(DEFAULT_CONFIG_PATH), help="path to config.toml")
    parser.add_argument("--home", default=str(OXIDEX_HOME), help="OXIDEX_HOME (default: $OXIDEX_HOME or ~/.oxidex)")
    parser.add_argument("--governor-path", default=None, help="default: <home>/logs/rate-governor.json")
    parser.add_argument("--manifest-log", default=None, help="default: <home>/logs/model-fix-requests/manifest.log")
    parser.add_argument("--tags-found-log", default=None, help="default: <home>/logs/tags-found.log")
    parser.add_argument("--json", action="store_true", help="print the raw result as JSON instead of a report")
    args = parser.parse_args(argv)

    home = Path(args.home)
    result = run_all_gates(
        config_path=Path(args.config),
        governor_path=Path(args.governor_path) if args.governor_path else home / "logs" / "rate-governor.json",
        manifest_path=Path(args.manifest_log) if args.manifest_log
        else home / "logs" / "model-fix-requests" / "manifest.log",
        tags_found_log_path=Path(args.tags_found_log) if args.tags_found_log else home / "logs" / "tags-found.log",
        disk_path=home,
    )

    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print(format_report(result))
    return 0 if result["overall_passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
