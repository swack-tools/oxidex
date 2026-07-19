#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""JPEG tag support matrix: empirical exiftool <-> oxidex read/write testing.

For every ExifTool-writable JPEG tag (manifest built from `exiftool -listx`):
  READ test:  exiftool writes the tag into a fresh JPEG -> oxidex -j reads it
              -> value compared (normalized).
  WRITE test: oxidex writes the tag into a fresh JPEG -> oxidex -j reads it
              back AND exiftool -j -G1 reads it -> values compared.

Emits <TAGMATRIX_WORK>/results.json (default: <system temp dir>/oxidex-tagmap)
for report generation.

Usage: uv run scripts/jpeg_tag_matrix.py [--only-group GROUP] [--limit N]
"""

import argparse
import json
import os
import re
import shutil
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

_REPO = Path(__file__).resolve().parent.parent
EXIFTOOL = os.environ.get("EXIFTOOL", "exiftool")
OXIDEX = os.environ.get("OXIDEX", str(_REPO / "target/release/oxidex"))
WORK = Path(os.environ.get("TAGMATRIX_WORK")
           or (Path(tempfile.gettempdir()) / "oxidex-tagmap"))
MANIFEST = WORK / "exiftool_jpeg_tags.json"
BASE = Path(os.environ.get("TAGMATRIX_BASE",
                           str(_REPO / "tests/fixtures/jpeg/tag_matrix_base.jpg")))
RESULTS = WORK / "results.json"

# EXIF family-1 groups whose tags oxidex prefixes with the same family-1 name.
EXIF_GROUPS = {"IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD", "SubIFD"}

# exiftool 13.55 itself serializes this tag with a malformed value offset
# (ASCII "1.5\0" in the offset field), which poisons the whole file for
# oxidex (drops the entire EXIF block) and aborts subsequent exiftool write
# chunks.  Excluded from batch writes; tested individually only.
BATCH_POISON = {"IFD0:GeoTiffDoubleParams"}


def run(cmd, timeout=30):
    """Run a command, returning (exit_code, stdout, stderr).

    `cmd` is always a list built from EXIFTOOL/OXIDEX (local tool paths,
    developer/CI-controlled env vars) plus fixed flags or values drawn from
    the manifest this same run generated locally from `exiftool -listx` --
    never from untrusted network input. No shell is invoked (no
    shell=True), so shell metacharacters in any argument cannot be
    interpreted; this is standard argv-list subprocess usage, not string
    concatenation into a shell command.
    """
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)  # nosec B603 # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit.dangerous-subprocess-use-audit,python.lang.security.audit.dangerous-subprocess-use-tainted-env-args.dangerous-subprocess-use-tainted-env-args
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return -1, "", "TIMEOUT"
    except Exception as e:  # noqa: BLE001
        return -2, "", str(e)


def exiftool_json(path):
    code, out, _ = run([EXIFTOOL, "-j", "-G1", "-charset", "utf8", str(path)])
    if code != 0 or not out.strip():
        return {}
    try:
        return json.loads(out)[0]
    except (json.JSONDecodeError, IndexError):
        return {}


def oxidex_json(path):
    # -e (exiftool-compat) gives PrintConv-style values closest to exiftool -j -G1
    code, out, err = run([OXIDEX, "-j", "-e", str(path)])
    if code != 0 or not out.strip():
        return None, err
    try:
        return json.loads(out)[0], None
    except (json.JSONDecodeError, IndexError):
        return None, "unparseable JSON"


# ---------------------------------------------------------------- value compare

RATIONAL_RE = re.compile(r"^(-?\d+)/(-?\d+)$")


def _as_float(s):
    s = str(s).strip()
    m = RATIONAL_RE.match(s)
    if m and int(m.group(2)) != 0:
        return int(m.group(1)) / int(m.group(2))
    try:
        return float(s)
    except ValueError:
        return None


def _norm_str(s):
    return re.sub(r"\s+", " ", str(s).strip()).lower()


def values_match(expected, actual):
    """Lenient comparison: exact, numeric (incl. rationals), date, unit-suffix."""
    if expected is None or actual is None:
        return False
    es, as_ = str(expected).strip(), str(actual).strip()
    if es == as_:
        return True
    if _norm_str(es) == _norm_str(as_):
        return True
    ef, af = _as_float(es), _as_float(as_)
    if ef is not None and af is not None:
        if ef == af:
            return True
        denom = max(abs(ef), abs(af), 1e-9)
        if abs(ef - af) / denom < 1e-3:
            return True
    # numeric with unit suffix, e.g. "10.5 m" vs "10.5"
    m = re.match(r"^(-?[\d.]+(?:/\d+)?)\s*\D*$", as_)
    if ef is not None and m:
        af2 = _as_float(m.group(1))
        if af2 is not None and abs(ef - af2) / max(abs(ef), 1e-9) < 1e-3:
            return True
    m = re.match(r"^(-?[\d.]+(?:/\d+)?)\s*\D*$", es)
    if af is not None and m:
        ef2 = _as_float(m.group(1))
        if ef2 is not None and abs(af - ef2) / max(abs(af), 1e-9) < 1e-3:
            return True
    # single-letter enum abbreviation vs PrintConv expansion ("N" <-> "North")
    if len(es) == 1 and as_ and as_[0].upper() == es.upper():
        return True
    if len(as_) == 1 and es and es[0].upper() == as_.upper():
        return True
    # dates: normalize separators (incl. T vs space), drop subseconds/timezone
    dnorm = lambda s: re.sub(r"[-:tT ]", ":", s).split("+")[0].split(".")[0].strip()  # noqa: E731
    if re.search(r"\d{4}[:-]\d{2}[:-]\d{2}", es) and dnorm(es) == dnorm(as_):
        return True
    # list vs scalar (single-element)
    return False


# ----------------------------------------------------------- key mapping rules
# NOTE: filled in from read/write path exploration; see docs in report.


def oxidex_read_keys(tag):
    """Candidate keys under which oxidex -j may expose this exiftool tag."""
    g, n = tag["group"], tag["name"]
    keys = []
    if g == "InteropIFD":
        keys += [f"EXIF:{n}", f"InteropIFD:{n}"]  # oxidex hardcodes EXIF: for interop
    elif g in EXIF_GROUPS:
        keys.append(f"{g}:{n}")
    elif g.startswith("XMP"):
        keys += [f"XMP:{n}", f"{g}:{n}"]  # dc/xmp namespaces flatten to XMP:
    elif g == "IPTC":
        keys.append(f"IPTC:{n}")
    elif g == "Photoshop":
        keys += [f"Photoshop:{n}", f"IPTC:{n}"]  # IRB parsed by IPTC parser
    elif g == "JFIF":
        keys.append(f"JFIF:{n}")
    else:
        keys.append(f"{g}:{n}")
    keys.append(n)  # bare-name fallback
    return keys


def oxidex_write_keys(tag):
    """Candidate -KEY=VALUE spellings for the oxidex CLI, tried in order.

    Write routing (validator.rs separate_by_ifd) only honors IFD0:/IFD1:/
    ExifIFD:/GPS:/EXIF: prefixes; EXIF: lands in IFD0 (wrong IFD for ExifIFD
    tags) so we use the exact family-1 prefix only. Other families are
    dropped silently — one spelling suffices to prove NOT_WRITTEN.
    """
    g, n = tag["group"], tag["name"]
    if g in EXIF_GROUPS:
        return [f"{g}:{n}"]
    if g.startswith("XMP"):
        return [f"XMP:{n}"]
    return [f"{g}:{n}"]


def find_in_json(data, keys):
    for k in keys:
        if k in data:
            return k, data[k]
    return None, None


def find_in_exiftool_json(data, tag, strict_group=False):
    """Find tag in exiftool -j -G1 output (exact group:name, then name-only).

    strict_group: require the exact family-1 group — used for write-test
    read-back so a tag written into the wrong IFD doesn't count as success.
    """
    k = f"{tag['group']}:{tag['name']}"
    if k in data:
        return data[k]
    if strict_group and tag["group"] in EXIF_GROUPS:
        return None
    for key, v in data.items():
        if key.split(":", 1)[-1] == tag["name"]:
            return v
    return None


# ------------------------------------------------------------------ read phase


def read_test_single(tag):
    """Isolated read test: exiftool writes ONLY this tag to a fresh base."""
    with tempfile.TemporaryDirectory() as td:
        img = Path(td) / "t.jpg"
        shutil.copy(BASE, img)
        run([EXIFTOOL, "-m", "-q", "-overwrite_original",
             f"-{tag['group']}:{tag['name']}={tag['sample']}", str(img)],
            timeout=60)
        et = exiftool_json(img)
        et_val = find_in_exiftool_json(et, tag)
        if et_val is None:
            return {"read": "NO_SAMPLE", "et_val": None}
        ox, oxerr = oxidex_json(img)
        if ox is None:
            return {"read": "OXIDEX_PARSE_FAIL", "et_val": et_val,
                    "read_detail": (oxerr or "")[:200]}
        k, v = find_in_json(ox, oxidex_read_keys(tag))
        if k is None:
            return {"read": "MISSING", "et_val": et_val}
        if values_match(et_val, v) or values_match(tag["sample"], v):
            return {"read": "OK", "ox_key": k, "ox_val": v, "et_val": et_val}
        return {"read": "MISMATCH", "ox_key": k, "ox_val": v, "et_val": et_val}


def read_test_group(tags):
    """Batch-write all tags of one group with exiftool, then oxidex-read once."""
    results = {}
    with tempfile.TemporaryDirectory() as td:
        img = Path(td) / "t.jpg"
        shutil.copy(BASE, img)
        # chunk writes to keep argv sane; -m tolerates minor per-tag issues
        chunk = 80
        batch_tags = [t for t in tags if key_of(t) not in BATCH_POISON]
        for i in range(0, len(batch_tags), chunk):
            args = [EXIFTOOL, "-m", "-q", "-overwrite_original"]
            for t in batch_tags[i : i + chunk]:
                args.append(f"-{t['group']}:{t['name']}={t['sample']}")
            args.append(str(img))
            run(args, timeout=120)
        et = exiftool_json(img)
        ox, oxerr = oxidex_json(img)
        for t in tags:
            et_val = find_in_exiftool_json(et, t)
            if et_val is None:
                results[key_of(t)] = {"read": "NO_SAMPLE", "et_val": None}
                continue
            if ox is None:
                results[key_of(t)] = {"read": "OXIDEX_PARSE_FAIL", "et_val": et_val,
                                      "read_detail": (oxerr or "")[:200]}
                continue
            k, v = find_in_json(ox, oxidex_read_keys(t))
            if k is None:
                results[key_of(t)] = {"read": "MISSING", "et_val": et_val}
            elif values_match(et_val, v) or values_match(t["sample"], v):
                results[key_of(t)] = {"read": "OK", "ox_key": k, "ox_val": v,
                                      "et_val": et_val}
            else:
                results[key_of(t)] = {"read": "MISMATCH", "ox_key": k, "ox_val": v,
                                      "et_val": et_val}
    return results


# ----------------------------------------------------------------- write phase


def write_test_tag(tag):
    """oxidex writes the tag -> oxidex reads back -> exiftool reads back."""
    res = {"write": "ERROR", "detail": ""}
    for wkey in oxidex_write_keys(tag):
        with tempfile.TemporaryDirectory() as td:
            img = Path(td) / "t.jpg"
            shutil.copy(BASE, img)
            code, out, err = run([OXIDEX, f"-{wkey}={tag['sample']}", str(img)])
            # oxidex sometimes prints "Error: ..." yet exits 0 — treat as error
            errtext = (err + out).strip()
            if code != 0 or "Error:" in errtext:
                res = {"write": "ERROR", "wkey": wkey, "detail": errtext[:200]}
                continue
            ox = oxidex_json(img)[0]
            et = exiftool_json(img)
            et_val = find_in_exiftool_json(et, tag, strict_group=True) if et else None
            ox_val = (find_in_json(ox, oxidex_read_keys(tag))[1]
                      if ox is not None else None)
            if not et:
                res = {"write": "CORRUPTS_FILE", "wkey": wkey,
                       "detail": "exiftool cannot parse output file"}
                continue
            ox_ok = ox_val is not None and values_match(tag["sample"], ox_val)
            et_ok = et_val is not None and values_match(tag["sample"], et_val)
            if ox_ok and et_ok:
                return {"write": "OK", "wkey": wkey, "ox_val": ox_val,
                        "et_val": et_val}
            if et_ok and not ox_ok:
                res = {"write": "READBACK_BROKEN", "wkey": wkey,
                       "detail": f"exiftool sees {et_val!r}, oxidex sees {ox_val!r}"}
            elif ox_ok and not et_ok:
                res = {"write": "INTEROP_BROKEN", "wkey": wkey,
                       "detail": f"oxidex reads back {ox_val!r} but exiftool sees {et_val!r}"}
            elif ox_val is not None or et_val is not None:
                res = {"write": "VALUE_MISMATCH", "wkey": wkey,
                       "detail": f"wrote {tag['sample']!r}; oxidex={ox_val!r} exiftool={et_val!r}"}
            else:
                res = {"write": "NOT_WRITTEN", "wkey": wkey,
                       "detail": ("exit 0 but tag absent on read-back; stderr: "
                                  + errtext[:150])}
    return res


def key_of(t):
    return f"{t['group']}:{t['name']}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only-group")
    ap.add_argument("--limit", type=int)
    ap.add_argument("--skip-write", action="store_true")
    ap.add_argument("--reread", action="store_true",
                    help="redo READ phase only; merge into existing results.json")
    ap.add_argument("--workers", type=int, default=8)
    args = ap.parse_args()

    manifest = json.loads(MANIFEST.read_text())
    tags = [t for t in manifest["tags"] if t.get("writable")]
    if args.only_group:
        tags = [t for t in tags if t["group"] == args.only_group]
    if args.limit:
        tags = tags[: args.limit]

    by_group = {}
    for t in tags:
        by_group.setdefault(t["group"], []).append(t)

    print(f"Testing {len(tags)} tags across {len(by_group)} groups", flush=True)

    results = {}
    if args.reread and RESULTS.exists():
        results = json.loads(RESULTS.read_text())
        args.skip_write = True

    # READ phase 1: one batch per group, groups in parallel
    read_res = {}
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = {ex.submit(read_test_group, ts): g for g, ts in by_group.items()}
        for fut in futs:
            read_res.update(fut.result())
    print("READ batch phase done", flush=True)

    # READ phase 2: individually retest every non-OK tag so one poison tag /
    # aborted chunk / mandatory-tag interaction can't contaminate a group.
    retest = [t for t in tags
              if read_res.get(key_of(t), {}).get("read") in
              ("MISSING", "MISMATCH", "NO_SAMPLE", "OXIDEX_PARSE_FAIL")]
    print(f"READ retest phase: {len(retest)} tags individually", flush=True)
    with ThreadPoolExecutor(max_workers=args.workers) as ex:
        futs = {ex.submit(read_test_single, t): t for t in retest}
        done = 0
        for fut, t in futs.items():
            single = fut.result()
            batch_status = read_res[key_of(t)].get("read")
            if single["read"] != batch_status:
                single["read_batch"] = batch_status
            read_res[key_of(t)] = single
            done += 1
            if done % 300 == 0:
                print(f"  retest {done}/{len(retest)}", flush=True)
    for t in tags:
        results.setdefault(key_of(t), {})
        # drop stale read fields before merging fresh read results
        for f in ("read", "read_batch", "read_detail", "ox_key", "ox_val", "et_val"):
            results[key_of(t)].pop(f, None)
        results[key_of(t)].update(read_res.get(key_of(t), {}))
    print("READ phase done", flush=True)

    # WRITE phase: per-tag isolation, parallel
    if not args.skip_write:
        with ThreadPoolExecutor(max_workers=args.workers) as ex:
            futs = {ex.submit(write_test_tag, t): t for t in tags}
            done = 0
            for fut, t in futs.items():
                results.setdefault(key_of(t), {}).update(fut.result())
                done += 1
                if done % 200 == 0:
                    print(f"  write {done}/{len(tags)}", flush=True)

    # attach manifest info
    for t in tags:
        r = results.setdefault(key_of(t), {})
        r["group"], r["name"], r["sample"] = t["group"], t["name"], t["sample"]
        r["type"] = t.get("type")
        r["protected"] = t.get("protected", False)

    RESULTS.write_text(json.dumps(results, indent=1))
    counts = {}
    for r in results.values():
        counts[(r.get("read"), r.get("write"))] = counts.get((r.get("read"), r.get("write")), 0) + 1
    for (rd, wr), n in sorted(counts.items(), key=lambda x: -x[1]):
        print(f"  read={rd:<18} write={wr!s:<18} {n}")
    print(f"Results: {RESULTS}")


if __name__ == "__main__":
    main()
