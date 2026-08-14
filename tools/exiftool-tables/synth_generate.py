#!/usr/bin/env python3
"""Step 28 corpus-synthesis: generate + round-trip-verify a subset of the
"synthesizable" tables from synth_classify.py's output.

Field data comes from src/exiftool_tables/binary_tables.rs (ALREADY verified
against ExifTool by `just verify-tables`), not re-derived from dump_tables.pl
JSON -- see rust_fields.py's docstring for why.

Per table:
  1. Copy a corpus carrier file into a scratch working copy.
  2. WRITE every eligible field (excludes SubDirectory pointers, and fields
     the transcription itself flags as depending on a ValueConv/RawConv/
     Condition/Hook it didn't reproduce -- writing a raw number under one of
     those and comparing it against a converted read would not be measuring
     what we intend) with the pinned exiftool, using the global `-n` flag to
     write the RAW (pre-PrintConv) value -- this sidesteps ExifTool's
     PrintConvInv/BITMASK-string-list syntax entirely and lets every field's
     sample be a plain number regardless of what its real Perl PrintConv is.
  3. ROUND TRIP: re-read the SAME file with `exiftool -n` (also raw) and
     confirm every attempted field reads back as the value that was written.
     This isolates "did ExifTool itself accept and store this write" from
     anything oxidex does.
  4. OXIDEX CHECK: read the file with `exiftool` (PrintConv'd, default) and
     with `oxidex -j -e` (also PrintConv'd -- oxidex's --no-print-conv flag
     was empirically found not to suppress conversion for MakerNote tags on
     this build, so the PrintConv'd form is what's actually comparable
     between the two tools) and lenient-match them, porting jpeg-tag-matrix's
     `values_match` (numeric tolerance, date normalization, single-letter
     enum abbreviation).

A table that gets "0 image files updated" back from exiftool (as opposed to
an error) is recorded as WRITE_NOOP, not WRITE_FAILED: it means this
PARTICULAR carrier file doesn't have a MakerNote block this table's offsets
land inside (e.g. an older camera body missing a newer sub-table), a
carrier-selection limitation, not a table-writability problem.
"""
import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from synth_carriers import CARRIER_MAP  # noqa: E402
from synth_rust_fields import parse_table_fields, is_scalar_writable  # noqa: E402

VENDOR_FILE = {
    "Canon": "CanonEOS_R8.jpg",
    "Nikon": "NikonZ8.jpg",
    "Sony": "SonyILCE-7RM4.jpg",
    "Pentax": "PentaxK-5.jpg",
    "Panasonic": "PanasonicDC-G9.jpg",
    "FujiFilm": "FujiFilmX-T5.jpg",
    "Olympus": "OlympusOM-1MarkII.jpg",
    "Samsung": "SamsungNX1.jpg",
}
# Fallback bodies to retry a table on if the primary carrier no-ops (older
# vs newer body -- some tables only exist on one generation).
VENDOR_FILE_FALLBACKS = {
    "Canon": ["CanonEOS-1D.jpg", "CanonEOS_R6m2.jpg"],
    "Nikon": ["NikonD500.jpg", "NikonD6.jpg"],
    "Sony": ["SonyDSLR-A100.jpg", "SonyILCE-9.jpg"],
    "FujiFilm": ["FujiFilmX-E1.jpg", "FujiFilmX-H2S.jpg"],
    "Olympus": ["OlympusE-M1.jpg", "OlympusE-M1X.jpg"],
}


def carrier_candidates(module: str, corpus: Path) -> list[Path]:
    """Ordered list of carrier files to try for this module, primary first."""
    entry = CARRIER_MAP.get(module)
    if entry is None:
        return []
    kind, target, _confidence, _note = entry
    if kind == "none":
        return []
    if kind == "vendor_dir":
        d = corpus / target
        out = []
        chosen = VENDOR_FILE.get(target)
        if chosen and (d / chosen).is_file():
            out.append(d / chosen)
        for fb in VENDOR_FILE_FALLBACKS.get(target, []):
            p = d / fb
            if p.is_file() and p not in out:
                out.append(p)
        if not out:
            files = sorted(d.glob("*.jpg"))
            out = files[:1]
        return out
    if kind == "file":
        if target.startswith("<"):
            candidates = sorted(corpus.glob("*.jpg"))
            return candidates[:1]
        p = corpus / target
        return [p] if p.is_file() else []
    return []


STR_TYPES = ("Str", "Undef")
RAT_TYPES = ("Float", "Double", "Rational64u", "Rational64s")
MAX_COUNT = 32
BAD_ENUM_LABELS = {"n/a", "unknown", "none", "off", "auto", ""}


def make_sample(f) -> str | None:
    if f.count > MAX_COUNT:
        return None  # large array/blob, out of scope for this harness
    if f.enum_pairs:
        for k, label in f.enum_pairs:
            if label.strip().lower() not in BAD_ENUM_LABELS:
                return k
        return f.enum_pairs[0][0]
    fmt = f.format
    if any(fmt.startswith(p) for p in STR_TYPES):
        return "3" if f.count == 1 else " ".join(["3"] * f.count)  # keep numeric+short; strings still accept digits
    scalar = "1.5" if any(fmt.startswith(p) for p in RAT_TYPES) else "3"
    return " ".join([scalar] * f.count) if f.count > 1 else scalar


# ---------------- lenient value matching (port of jpeg-tag-matrix's) -------
RATIONAL_RE = re.compile(r"^(-?\d+)/(-?\d+)$")


def as_float(s: str):
    s = s.strip()
    m = RATIONAL_RE.match(s)
    if m:
        num, den = int(m.group(1)), int(m.group(2))
        return num / den if den else None
    try:
        return float(s)
    except ValueError:
        return None


def values_match(expected: str, actual: str) -> bool:
    e, a = expected.strip(), actual.strip()
    if e == a:
        return True
    if " ".join(e.split()).lower() == " ".join(a.split()).lower():
        return True
    ef, af = as_float(e), as_float(a)
    if ef is not None and af is not None:
        if ef == af:
            return True
        denom = max(abs(ef), abs(af), 1e-9)
        if abs(ef - af) / denom < 1e-3:
            return True
    m = re.match(r"^(-?[\d.]+(?:/\d+)?)\s*\D*$", a)
    if ef is not None and m:
        af2 = as_float(m.group(1))
        if af2 is not None and abs(ef - af2) / max(abs(ef), 1e-9) < 1e-3:
            return True
    if len(e) == 1 and a and a[0].lower() == e[0].lower():
        return True
    if len(a) == 1 and e and e[0].lower() == a[0].lower():
        return True
    return False


def run(cmd: list[str], timeout=60) -> tuple[int, str, str]:
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return -1, "", "TIMEOUT"


def read_json(cmd: list[str]) -> dict:
    code, out, _err = run(cmd, timeout=30)
    if code != 0 or not out.strip():
        return {}
    try:
        arr = json.loads(out)
        return arr[-1] if arr else {}
    except json.JSONDecodeError:
        return {}


def find_key(data: dict, module: str, name: str):
    for k in (f"{module}:{name}", name):
        if k in data:
            return k, data[k]
    for k, v in data.items():
        if k.split(":", 1)[-1] == name:
            return k, v
    return None, None


def val_str(v) -> str:
    return v if isinstance(v, str) else json.dumps(v)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary-tables", type=Path, required=True)
    ap.add_argument("--corpus", type=Path, required=True)
    ap.add_argument("--select", type=Path, required=True)
    ap.add_argument("--exiftool", required=True)
    ap.add_argument("--oxidex", required=True)
    ap.add_argument("--work", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()

    args.work.mkdir(parents=True, exist_ok=True)
    rs_text = args.binary_tables.read_text()
    exiftool_cmd = [args.exiftool]

    selection = [
        tuple(line.strip().split(":", 1))
        for line in args.select.read_text().splitlines()
        if line.strip() and not line.startswith("#")
    ]

    report = []
    for module, table in selection:
        row = {"module": module, "table": table}
        candidates = carrier_candidates(module, args.corpus)
        if not candidates:
            row["status"] = "NO_CARRIER"
            report.append(row)
            print(f"[{module}:{table}] NO_CARRIER", file=sys.stderr)
            continue

        parsed = parse_table_fields(rs_text, module, table)
        if parsed is None:
            row["status"] = "NOT_IN_BINARY_TABLES_RS"
            report.append(row)
            continue
        _default_fmt, fields = parsed
        eligible = [f for f in fields if is_scalar_writable(f)]
        tags = []
        for f in eligible:
            s = make_sample(f)
            if s is not None:
                tags.append((f.name, s))
        if not tags:
            row["status"] = "NO_ELIGIBLE_FIELDS"
            row["total_fields"] = len(fields)
            report.append(row)
            print(f"[{module}:{table}] NO_ELIGIBLE_FIELDS (of {len(fields)} total)", file=sys.stderr)
            continue
        row["attempted_tags"] = len(tags)
        row["total_fields"] = len(fields)

        # Try each candidate carrier in order until one accepts the write
        # (a NOOP means THIS body's MakerNote doesn't have the block, not
        # that the table is unwritable -- see module docstring).
        code = out = err = None
        carrier = None
        for cand in candidates:
            carrier = cand
            work_file = args.work / f"{module}_{table}{carrier.suffix}"
            shutil.copy(carrier, work_file)
            write_args = [f"-{module}:{name}={sample}" for name, sample in tags]
            code, out, err = run(
                exiftool_cmd + ["-n", "-m", "-overwrite_original"] + write_args + [str(work_file)],
                timeout=60,
            )
            noop = code == 0 and (
                "image files updated" not in out or re.search(r"\b0 image files updated\b", out)
            )
            if code == 0 and not noop:
                break  # got a real write, stop trying fallbacks
        row["carrier"] = str(carrier.relative_to(args.corpus))
        row["carrier_candidates_tried"] = len(candidates)
        row["write_exit"] = code
        row["write_stdout"] = out.strip()[:300]
        if code != 0:
            row["status"] = "WRITE_FAILED"
            row["write_stderr"] = err.strip()[:400]
            report.append(row)
            print(f"[{module}:{table}] WRITE_FAILED (exit {code}): {err.strip()[:150]}", file=sys.stderr)
            continue
        if "image files updated" not in out or re.search(r"\b0 image files updated\b", out):
            row["status"] = "WRITE_NOOP"
            row["write_stderr"] = err.strip()[:400]
            report.append(row)
            print(f"[{module}:{table}] WRITE_NOOP: {out.strip()[:150]}", file=sys.stderr)
            continue

        # round trip: raw re-read via exiftool -n
        et_raw = read_json(exiftool_cmd + ["-n", "-j", "-G1", str(work_file)])
        # PrintConv'd reads for the oxidex comparison
        et_pc = read_json(exiftool_cmd + ["-j", "-G1", "-charset", "utf8", str(work_file)])
        code, out, err = run([args.oxidex, "-j", "-e", str(work_file)], timeout=30)
        ox_pc = None
        ox_err = None
        if code == 0 and out.strip():
            try:
                arr = json.loads(out)
                ox_pc = arr[-1] if arr else {}
            except json.JSONDecodeError:
                ox_err = "unparseable JSON"
        else:
            ox_err = err[:300]

        tag_results = []
        for name, sample in tags:
            rk, rv = find_key(et_raw, module, name)
            rt_ok = rk is not None and values_match(sample, val_str(rv))
            entry = {
                "tag": name, "sample": sample,
                "exiftool_roundtrip": "OK" if rt_ok else ("MISMATCH" if rk else "NOT_WRITTEN"),
                "exiftool_raw_val": val_str(rv) if rv is not None else None,
            }
            if not rt_ok:
                entry["oxidex"] = "SKIPPED_NO_ROUNDTRIP"
            elif ox_pc is None:
                entry["oxidex"] = "OXIDEX_PARSE_FAIL"
                entry["oxidex_detail"] = ox_err
            else:
                pk, pv = find_key(et_pc, module, name)
                expected_pc = val_str(pv) if pv is not None else sample
                ok_, ov = find_key(ox_pc, module, name)
                if ok_ is None:
                    entry["oxidex"] = "MISSING"
                elif values_match(expected_pc, val_str(ov)):
                    entry["oxidex"] = "OK"
                    entry["oxidex_key"] = ok_
                else:
                    entry["oxidex"] = "MISMATCH"
                    entry["oxidex_val"] = val_str(ov)
                    entry["oxidex_key"] = ok_
                    entry["expected_printconv"] = expected_pc
            tag_results.append(entry)

        rt_ok_n = sum(1 for t in tag_results if t["exiftool_roundtrip"] == "OK")
        ox_ok_n = sum(1 for t in tag_results if t.get("oxidex") == "OK")
        row["status"] = "TESTED"
        row["roundtrip_ok"] = rt_ok_n
        row["oxidex_ok"] = ox_ok_n
        row["tags"] = tag_results
        report.append(row)
        print(
            f"[{module}:{table}] {len(tags)} attempted, {rt_ok_n} exiftool-roundtrip-ok, "
            f"{ox_ok_n} oxidex-ok",
            file=sys.stderr,
        )

    args.out.write_text(json.dumps(report, indent=2))

    tested = [r for r in report if r["status"] == "TESTED"]
    total_attempted = sum(r["attempted_tags"] for r in tested)
    total_rt_ok = sum(r["roundtrip_ok"] for r in tested)
    total_ox_ok = sum(r["oxidex_ok"] for r in tested)
    tables_lit = sum(1 for r in tested if r["oxidex_ok"] > 0)
    print("\n=== SUBSET SUMMARY ===", file=sys.stderr)
    print(f"tables selected:      {len(selection)}", file=sys.stderr)
    print(f"tables tested:        {len(tested)}", file=sys.stderr)
    other = {}
    for r in report:
        if r["status"] != "TESTED":
            other[r["status"]] = other.get(r["status"], 0) + 1
    print(f"tables not tested:    {other}", file=sys.stderr)
    print(f"tags attempted:       {total_attempted}", file=sys.stderr)
    print(f"tags exiftool-roundtrip-ok: {total_rt_ok}", file=sys.stderr)
    print(f"tags oxidex-ok:        {total_ox_ok}", file=sys.stderr)
    print(f"tables lit (>=1 oxidex-ok tag): {tables_lit} / {len(tested)}", file=sys.stderr)


if __name__ == "__main__":
    main()
