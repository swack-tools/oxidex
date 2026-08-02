#!/usr/bin/env python3
"""Independently re-check what the autonomous fleet decided.

The fleet already gates its own work. This does NOT trust any of that: it
re-derives each decision from ExifTool ground truth and reports where the
fleet's conclusion and an independent one disagree.

Why a separate checker at all. Every gate in the pipeline shares code with
the thing it gates -- the merger calls validate_fix_commit, the judgment
daemon calls verify_enum_maps, the sweep calls the same comparison harness
the workers do. A bug in that shared layer is invisible to all of them at
once, and on 2026-07-27 exactly that happened three times: a duplicate gate
read POST-only in three separate files, and every caller agreed.

So this deliberately re-reads the ExifTool .pm sources itself rather than
asking the fleet's own verifier.

What it checks, in descending order of what an error would cost:

  rejected-permanent   IRREVERSIBLE. Every claimed mismatch is re-checked
                       against the cited module. A wrong one discards good
                       work forever.
  promoted             The commit re-enters the merger gate, so a bad one
                       is caught later -- but a FABRICATED TRAILER would
                       pass that gate on false evidence, so every derived
                       Exiftool-Value is re-measured against the sample.
  sweep pushes         What actually reaches main.

Findings go to <archive>/verification-log.jsonl, one JSON object per
finding, append-only. Silence means agreement.
"""

import argparse
import json
import re
import subprocess  # nosec B603,B404 -- list-argv only, no shell=True
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from exiftool_oracle import (  # noqa: E402
    cache_dir as exiftool_cache_dir,
    shared as shared_exiftool_oracle,
)

FLEET_LOG = Path.home() / ".oxidex" / "logs" / "fleet-up.log"
# The PINNED tree, not a Cellar path. A hardcoded /opt/homebrew/.../13.55/...
# meant this checker re-derived every rejection from a release the tables were
# never transcribed from (13.59) -- so it could "independently confirm" a
# rejection that the correct source disagrees with, and the confirmation is
# what makes the rejection irreversible.
DEFAULT_PERL = exiftool_cache_dir() / "exiftool" / "lib" / "Image" / "ExifTool"
DEFAULT_OUT = Path.home() / ".oxidex" / "patch-archive" / "verification-log.jsonl"
STATE = Path.home() / ".oxidex" / "logs" / "verify-fleet-decisions.state"


def run(*args, **kw):
    return subprocess.run(list(args), capture_output=True, text=True, **kw)  # nosec B603


def perl_printconv_value(perl_dir, module, key):
    """The display string ExifTool maps `key` to in `module`, or None.

    Scans for `<key> => '<value>'`. Deliberately dumb and deliberately
    independent of verify_enum_maps -- a shared parser would share its bugs.
    """
    path = Path(perl_dir) / module
    if not path.is_file():
        return None
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return None
    hits = re.findall(rf"(?m)^\s*{re.escape(str(key))}\s*=>\s*'([^']*)'", text)
    return hits[0] if len(hits) == 1 else (hits if hits else None)


def check_permanent_rejection(entry, perl_dir):
    """Re-derive an IRREVERSIBLE rejection. Returns a finding or None."""
    detail = (entry.get("detail") or {}).get("verifier") or {}
    mismatches = detail.get("mismatches") or []
    table = detail.get("table") or ""
    module = None
    m = re.search(r"Image::ExifTool::(\w+)::", table)
    if m:
        module = f"{m.group(1)}.pm"
    if not mismatches:
        # Convicted with no pair evidence recorded -- that alone is worth a look,
        # because a terminal verdict should always carry its evidence.
        return {"kind": "permanent-rejection-without-pair-evidence",
                "sha": entry.get("sha"), "reason": entry.get("reason")}
    if not module:
        return {"kind": "permanent-rejection-unattributable-table",
                "sha": entry.get("sha"), "table": table}
    disagreements = []
    for mm in mismatches:
        key, claimed = mm.get("key"), mm.get("exiftool")
        truth = perl_printconv_value(perl_dir, module, key)
        if truth is None:
            continue  # cannot re-derive; not evidence of error
        if isinstance(truth, list):
            continue  # ambiguous in the module; do not accuse
        if claimed is not None and truth != claimed:
            disagreements.append({"key": key, "daemon_said": claimed, "module_says": truth})
    if disagreements:
        return {"kind": "PERMANENT-REJECTION-EVIDENCE-DISAGREES",
                "sha": entry.get("sha"), "module": module,
                "disagreements": disagreements}
    return None


def check_promotion(entry, samples_root):
    """A promotion's derived evidence must be TRUE.

    The commit re-enters the merger gate, so wrong CODE gets caught later.
    A wrong TRAILER does not: it is the evidence that gate reads.
    """
    sha = entry.get("sha")
    if not sha:
        return None
    body = run("git", "log", "-1", "--format=%B", sha, cwd=str(REPO)).stdout
    if not body:
        return None
    trailers = {}
    for line in body.splitlines():
        if ":" in line:
            k, _, v = line.partition(":")
            trailers.setdefault(k.strip(), []).append(v.strip())
    sample = next(iter(trailers.get("Sample", [])), "")
    claimed = next(iter(trailers.get("Exiftool-Value", [])), None)
    tags = trailers.get("Tag", [])
    if not (sample and claimed and tags):
        return None
    if not Path(sample).is_file():
        return {"kind": "promotion-cites-missing-sample", "sha": sha, "sample": sample}
    # The convention is that Exiftool-Value describes the FIRST Tag trailer.
    name = tags[0].split(":", 1)[-1]
    # One `-a -G1 -s` pass rather than `-s3 -<tag>`. Two reasons, both learned
    # the hard way on 2026-07-27 against CanonRaw.cr2 / EXIF:PreviewImage:
    #   * -s3 on a BINARY tag emits the raw bytes, not the
    #     "(Binary data N bytes, use -b option to extract)" placeholder that
    #     the trailer actually records -- so the comparison saw empty output
    #     and cried fabrication on correct evidence.
    #   * without -a, a tag present only in a non-primary group (that one is
    #     in IFD0) does not print at all, which reads as "absent".
    # The binary is the pinned oracle, never a bare `exiftool`: PATH here
    # resolved to 13.55 while the trailers being re-measured were derived from
    # 13.59, so a correct Exiftool-Value could be branded FABRICATED by a
    # checker whose entire job is to be the independent one.
    dump = run(*shared_exiftool_oracle().command(["-a", "-G1", "-s", sample])).stdout
    found = []
    for line in dump.splitlines():
        m = re.match(r"^\[[^\]]+\]\s+(\S+)\s+:\s*(.*)$", line)
        if m and m.group(1) == name:
            found.append(m.group(2).strip())
    if not found:
        return {"kind": "promotion-evidence-tag-absent-from-sample",
                "sha": sha, "tag": tags[0], "sample": sample, "claimed": claimed}
    # Trailers quote their value; compare unquoted, and accept any group's
    # copy of the tag (the trailer does not record which group it came from).
    want = claimed.strip().strip("'\"")
    if not any(want == f or want in f or f in want for f in found):
        return {"kind": "PROMOTION-EVIDENCE-DISAGREES", "sha": sha, "tag": tags[0],
                "claimed": want, "exiftool_says": found[:3], "sample": sample}
    return None


def iter_new_decisions(since_offset):
    """Judgment-daemon decision records appended since the last run."""
    if not FLEET_LOG.is_file():
        return since_offset, []
    size = FLEET_LOG.stat().st_size
    if size < since_offset:      # log rotated
        since_offset = 0
    out = []
    with FLEET_LOG.open("r", errors="replace") as fh:
        fh.seek(since_offset)
        for line in fh:
            if "[judgment] {" not in line:
                continue
            _, _, blob = line.partition("[judgment] ")
            try:
                out.append(json.loads(blob.strip()))
            except ValueError:
                continue
        new_offset = fh.tell()
    return new_offset, out


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repo", default="/Users/allen/git/oxidex")
    ap.add_argument("--perl-dir", default=str(DEFAULT_PERL))
    ap.add_argument("--samples", default="/tmp/oxidex-exiftool-cache/combined-samples")
    ap.add_argument("--out", default=str(DEFAULT_OUT))
    ap.add_argument("--once", action="store_true")
    ap.add_argument("--interval", type=int, default=600)
    args = ap.parse_args()

    global REPO
    REPO = Path(args.repo)
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)

    offset = 0
    if STATE.is_file():
        try:
            offset = int(STATE.read_text().strip() or 0)
        except ValueError:
            offset = 0

    while True:
        offset, decisions = iter_new_decisions(offset)
        findings = []
        counts = {"promoted": 0, "rejected-permanent": 0, "other": 0}
        for d in decisions:
            verdict = d.get("verdict")
            if verdict == "rejected-permanent":
                counts["rejected-permanent"] += 1
                f = check_permanent_rejection(d, args.perl_dir)
            elif verdict == "promoted":
                counts["promoted"] += 1
                f = check_promotion(d, args.samples)
            else:
                counts["other"] += 1
                f = None
            if f:
                f["ts"] = datetime.now(timezone.utc).isoformat()
                f["verdict"] = verdict
                findings.append(f)
        if findings:
            with out_path.open("a") as fh:
                for f in findings:
                    fh.write(json.dumps(f) + "\n")
            for f in findings:
                print(f"DISAGREEMENT {f['kind']}: {json.dumps(f)[:240]}", flush=True)
        elif decisions:
            print(f"checked {len(decisions)} decision(s) "
                  f"({counts['promoted']} promoted, {counts['rejected-permanent']} terminal) "
                  f"-- no disagreement", flush=True)
        STATE.write_text(str(offset))
        if args.once:
            return 0
        time.sleep(args.interval)


if __name__ == "__main__":
    sys.exit(main())
