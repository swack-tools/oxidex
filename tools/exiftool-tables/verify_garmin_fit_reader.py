#!/usr/bin/env python3
"""Compare the generated Garmin FIT reader with the pinned native ExifTool.

One fixture per distinct protocol, base-type or conversion behavior (plus the
pinned tree's real `t/images/Garmin.fit`), not one per tag. Each fixture is
read by the source-pinned ExifTool and by a freshly built OxiDex, both with
`-j -a -G1`, in print and no-print-conversion modes, and projected onto the
groups the FIT protocol reports (every generated FIT family-1 group, the
synthesized `Unknown<num>` groups, `File:ProtocolVersion` and `ExifTool:*`).
Warnings are compared as a multiset of texts: ExifTool's JSON writer keeps
one entry per key, so its text output supplies every native occurrence.

A projected identity is credited as observed only when both sides report it
with the same value. A native identity OxiDex does not report is MISSING and
must be explained by a declared refusal class for that fixture; a value
difference or an OxiDex-only identity fails the run. Observed reading is the
only claim: this instrument performs no writes.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import struct
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
sys.path.insert(0, str(HERE))

import exiftool_oracle  # noqa: E402
import instrument  # noqa: E402

SCHEMA = "oxidex_garmin_fit_read_evidence_v1"
LEDGER = HERE / "garmin_fit_ledger.json"

# FIT base type ids (Garmin.pm %baseType).
ENUM, SINT8, UINT8, SINT16, UINT16, SINT32, UINT32, STRING = 0x00, 0x01, 0x02, 0x83, 0x84, 0x85, 0x86, 0x07
FLOAT32, FLOAT64, UINT8Z, UINT16Z, UINT32Z, BYTE, SINT64, UINT64, UINT64Z = 0x88, 0x89, 0x0A, 0x8B, 0x8C, 0x0D, 0x8E, 0x8F, 0x90
FORMATS = {ENUM: "B", SINT8: "b", UINT8: "B", SINT16: "h", UINT16: "H", SINT32: "i", UINT32: "I",
           FLOAT32: "f", FLOAT64: "d", UINT8Z: "B", UINT16Z: "H", UINT32Z: "I", SINT64: "q",
           UINT64: "Q", UINT64Z: "Q"}
TS = 1_100_000_000  # FIT epoch seconds (2024-11-09)


class Fit:
    """Build a FIT stream record by record (CRC bytes present, never checked)."""

    def __init__(self, header_size=12, protocol=0x10):
        self.header_size, self.protocol, self.records = header_size, protocol, bytearray()

    def define(self, local, message, fields, big=False, dev=None):
        order = ">" if big else "<"
        flags = 0x40 | local | (0x20 if dev else 0)
        self.records += bytes([flags, 0, 1 if big else 0]) + struct.pack(order + "H", message)
        self.records += bytes([len(fields)])
        for num, size, base in fields:
            self.records += bytes([num, size, base])
        if dev:
            self.records += bytes([len(dev)])
            for num, size, index in dev:
                self.records += bytes([num, size, index])
        return self

    def data(self, local, payload, compressed_offset=None):
        if compressed_offset is None:
            self.records += bytes([local & 0x0F])
        else:
            self.records += bytes([0x80 | (local << 5) | compressed_offset])
        self.records += payload
        return self

    def raw(self, payload):
        self.records += payload
        return self

    def build(self, truncate=0):
        header = bytes([self.header_size, self.protocol]) + struct.pack("<H", 2132)
        header += struct.pack("<I", len(self.records)) + b".FIT"
        if self.header_size == 14:
            header += b"\0\0"
        body = bytes(self.records) + b"\0\0"
        blob = header + body
        return blob[: len(blob) - truncate] if truncate else blob


def pack(values, big=False):
    order = ">" if big else "<"
    out = b""
    for base, value in values:
        if base == STRING or base == BYTE:
            out += value
        elif isinstance(value, (list, tuple)):
            out += struct.pack(order + FORMATS[base] * len(value), *value)
        else:
            out += struct.pack(order + FORMATS[base], value)
    return out


def session(big=False):
    # Session (18): AvgHeartRate(16) "$val bpm"; AvgFractionalCadence(92)
    # ValueConv /128 + PrintConv; Sport(5) enum; AvgLeftPowerPhase(116) list,
    # no conversion; MessageIndex(254) via Common; TimeStamp(253) via Common.
    fields = [(253, 4, UINT32), (254, 2, UINT16), (5, 1, ENUM), (16, 1, UINT8), (92, 1, UINT8), (116, 4, UINT8)]
    payload = pack([(UINT32, TS), (UINT16, 0), (ENUM, 1), (UINT8, 87), (UINT8, 68), (UINT8, [1, 2, 3, 255])], big)
    return fields, payload


def cases():
    fixtures = {}
    fields, payload = session()
    fixtures["session-little.fit"] = Fit().define(0, 18, fields).data(0, payload).build()
    fields, payload = session(big=True)
    fixtures["session-big.fit"] = Fit().define(0, 18, fields, big=True).data(0, payload).build()
    fields, payload = session()
    second = pack([(UINT32, TS + 60), (UINT16, 1), (ENUM, 2), (UINT8, 99), (UINT8, 1), (UINT8, [9, 9, 9, 9])])
    fixtures["first-message-gate.fit"] = Fit().define(0, 18, fields).data(0, payload).data(0, second).build()
    fixtures["header-14.fit"] = Fit(header_size=14).define(0, 18, fields).data(0, payload).build()
    # An Unknown-flagged message (Activity, 34) contributes only its timestamp.
    fixtures["unknown-message-timestamp.fit"] = (
        Fit().define(0, 34, [(253, 4, UINT32), (1, 2, UINT16)])
        .data(0, pack([(UINT32, TS), (UINT16, 7)])).build())
    # An unlisted message number and an edge with no table (Pad, 105).
    fixtures["unlisted-and-synthesized.fit"] = (
        Fit().define(0, 60000, [(253, 4, UINT32)]).data(0, pack([(UINT32, TS)]))
        .define(1, 105, [(253, 4, UINT32)]).data(1, pack([(UINT32, TS + 5)])).build())
    # Compressed header: timestamp rolls from the previous full timestamp and
    # is reported through Common under the message's group, beside its fields.
    fixtures["compressed-timestamp.fit"] = (
        Fit().define(0, 34, [(253, 4, UINT32)]).data(0, pack([(UINT32, TS)]))
        .define(1, 20, [(3, 1, UINT8)]).data(1, pack([(UINT8, 70)]), compressed_offset=(TS + 3) & 0x1F)
        .build())
    # A field whose base type is not in %baseType warns, is left out of the
    # field list, and shifts every later read (its size still counts).
    fixtures["unknown-base-type.fit"] = (
        Fit().define(0, 18, [(253, 4, UINT32), (200, 1, 0x55), (16, 1, UINT8), (18, 1, UINT8)])
        .data(0, pack([(UINT32, TS), (UINT8, 50), (UINT8, 60), (UINT8, 70)])).build())
    # Invalid values: scalar sentinels dropped, sentinel lists kept.
    fixtures["invalid-values.fit"] = (
        Fit().define(0, 18, [(253, 4, UINT32), (16, 1, UINT8), (17, 1, UINT8), (116, 2, UINT8),
                             (110, 4, STRING), (19, 2, UINT16Z), (168, 4, FLOAT32)])
        .data(0, pack([(UINT32, TS), (UINT8, 255), (UINT8, 120), (UINT8, [255, 255]),
                       (STRING, b"\0abc"), (UINT16Z, 0), (FLOAT32, float("nan"))])).build())
    # Non-integral count: warns and skips the field.
    fixtures["bad-count.fit"] = (
        Fit().define(0, 18, [(253, 4, UINT32), (20, 3, UINT16), (16, 1, UINT8)])
        .data(0, pack([(UINT32, TS)]) + b"\1\2\3" + pack([(UINT8, 80)])).build())
    # Developer fields are sized and skipped in default mode.
    fixtures["developer-fields.fit"] = (
        Fit().define(0, 18, [(253, 4, UINT32), (16, 1, UINT8)], dev=[(0, 2, 0), (1, 3, 0)])
        .data(0, pack([(UINT32, TS), (UINT8, 81)]) + b"\1\2\3\4\5")
        .define(1, 19, [(16, 1, UINT8)]).data(1, pack([(UINT8, 82)])).build())
    # Float, string and exact 64-bit integer base types.
    fixtures["float-string-int64.fit"] = (
        Fit().define(0, 18, [(253, 4, UINT32), (168, 4, FLOAT32), (110, 8, STRING), (254, 8, UINT64)])
        .data(0, pack([(UINT32, TS), (FLOAT32, 3.7091979980468750), (STRING, b"Birding\0"),
                       (UINT64, 9007199254740993)])).build())
    # `byte` values are scalar references: binary placeholder, no conversion.
    fixtures["byte-field.fit"] = (
        Fit().define(0, 18, [(253, 4, UINT32), (16, 3, BYTE)])
        .data(0, pack([(UINT32, TS), (BYTE, b"\1\2\3")])).build())
    # RawConv / ValueConv / PrintConv chain on positions (Record 0/1).
    fixtures["position-conversions.fit"] = (
        Fit().define(0, 20, [(253, 4, UINT32), (0, 4, SINT32), (1, 4, SINT32), (2, 2, UINT16)])
        .data(0, pack([(UINT32, TS), (SINT32, 390_523_136), (SINT32, -1_397_758_848), (UINT16, 2_819)])).build())
    # Stream errors end the walk and keep what was read.
    fixtures["truncated.fit"] = Fit().define(0, 18, fields).data(0, payload).define(1, 19, [(16, 1, UINT8)]).raw(b"\1").build(truncate=2)
    fixtures["missing-definition.fit"] = Fit().define(0, 18, fields).data(0, payload).raw(b"\3\0").build()
    # A numeric conversion over a multi-element value: ExifTool interpolates
    # the joined list ("87 88 bpm"); the list domain is not compiled, so the
    # reader withholds the tag (declared below).
    fixtures["list-domain-refusal.fit"] = (
        Fit().define(0, 18, [(253, 4, UINT32), (16, 2, UINT8), (17, 1, UINT8)])
        .data(0, pack([(UINT32, TS), (UINT8, [87, 88]), (UINT8, 120)])).build())
    # DeveloperDataID (207) and FieldDescription (206) are Unknown-flagged,
    # so without -u their values are never captured and a developer field on
    # a later Session record is skipped silently by both tools.
    fixtures["developer-descriptions.fit"] = (
        Fit().define(0, 207, [(1, 4, BYTE), (3, 1, UINT8)])
        .data(0, pack([(BYTE, b"\xde\xad\xbe\xef"), (UINT8, 0)]))
        .define(1, 206, [(0, 1, UINT8), (1, 1, UINT8), (2, 1, UINT8), (3, 6, STRING), (8, 4, STRING)])
        .data(1, pack([(UINT8, 0), (UINT8, 0), (UINT8, UINT8), (STRING, b"speed\0"), (STRING, b"km/h")]))
        .define(2, 18, [(253, 4, UINT32), (16, 1, UINT8)], dev=[(0, 1, 0)])
        .data(2, pack([(UINT32, TS), (UINT8, 83)]) + b"\x2a").build())
    # Values with no exact numeric carrier: an unsigned 64-bit value above
    # i64::MAX and non-finite floats print as Perl text where no conversion
    # reads them.
    fixtures["text-only-values.fit"] = (
        Fit().define(0, 18, [(253, 4, UINT32), (254, 8, UINT64), (25, 4, FLOAT32), (116, 8, FLOAT32)])
        .data(0, pack([(UINT32, TS), (UINT64, 2**64 - 2), (FLOAT32, float("inf")),
                       (FLOAT32, [float("-inf"), 1.5])])).build())
    # A negative running timestamp: Perl masks its two's-complement value in
    # the compressed-header arithmetic.
    fixtures["negative-timestamp.fit"] = (
        Fit().define(0, 34, [(253, 4, SINT32)]).data(0, pack([(SINT32, -5)]))
        .define(1, 20, [(3, 1, UINT8)]).data(1, pack([(UINT8, 71)]), compressed_offset=7).build())
    return fixtures


# Native identities OxiDex deliberately does not report, by fixture, with the
# refusal class that explains each. Anything else missing fails the run.
EXPECTED_MISSING = {
    "list-domain-refusal.fit": {"Session:AvgHeartRate": "typed_conversion_list_domain"},
}


def load_pairs(text):
    """ExifTool's `-a -j` repeats keys; keep every occurrence in order."""
    return json.loads(text, object_pairs_hook=lambda pairs: [pairs])[0][0]


IDENTITY_GROUPS = {"System", "File", "Composite", "ExifTool"}
WARNING_KEY = re.compile(r"ExifTool:Warning(?: \(\d+\))?")


def fit_owned(key, groups):
    group = key.split(":", 1)[0]
    if WARNING_KEY.fullmatch(key):
        return False  # compared separately, as a multiset (see warnings_of)
    return (group in groups or (group.startswith("Unknown") and group[7:].isdigit())
            or key == "File:ProtocolVersion"
            or (group == "ExifTool" and key != "ExifTool:ExifToolVersion"))


def native_warnings(oracle, path):
    """Every native warning. ExifTool's JSON writer keeps one entry per key,
    so `-j -a` shows only the first; the text form lists each occurrence."""
    run = subprocess.run(oracle.command(["-config", "", "-a", "-G1", "-s", "-ExifTool:Warning", str(path)]),
                         text=True, capture_output=True, check=True)
    return sorted(line.split(":", 1)[1].strip() for line in run.stdout.splitlines()
                  if line.startswith("[ExifTool]") and ":" in line)


def oxidex_warnings(text):
    return sorted(value for key, value in load_pairs(text) if WARNING_KEY.fullmatch(key))


def project(text, groups):
    out = {}
    for key, value in load_pairs(text):
        if ":" in key and fit_owned(key, groups):
            out.setdefault(key, []).append(value)
    return {key: sorted(json.dumps(v, sort_keys=True) for v in values) for key, values in out.items()}


def stray_groups(text, native_text, groups):
    """OxiDex keys under a group neither FIT-owned, an identity group, nor
    reported natively (e.g. a family-0 `Garmin:` leak): EXTRA, not ignored."""
    native = {key.split(":", 1)[0] for key, _ in load_pairs(native_text) if ":" in key}
    return sorted(key for key, _ in load_pairs(text)
                  if ":" in key and not fit_owned(key, groups)
                  and key.split(":", 1)[0] not in IDENTITY_GROUPS | native)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exiftool-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--oxidex", type=Path, help="prebuilt binary; default builds one with Cargo")
    args = parser.parse_args()
    if args.out.exists():
        raise SystemExit("output exists; choose a new evidence directory")
    state = instrument.git_state(ROOT)
    overridden = instrument.refuse_if_dirty(state, "verify_garmin_fit_reader.py")
    oracle = exiftool_oracle.resolve_tree(args.exiftool_dir.resolve())
    if not oracle.verified or oracle.version != (ROOT / ".exiftool-version").read_text().strip():
        raise SystemExit("native oracle capability/version does not match the pin")
    ledger = json.loads(LEDGER.read_text())
    if ledger.get("module_state") == "absent":
        # Nothing to compare: the release has no FIT reader and the generated
        # protocol extracts nothing. Say so rather than credit or fail rows.
        raise SystemExit(f"Garmin module absent from ExifTool {ledger['source']['exiftool_version']} "
                         "(proven from its source tree); no FIT reader to compare")
    # Every family-1 group a FIT message can report under, withheld edges
    # included, so a refused message still counts native tags as MISSING:
    # each message table's group (Common's own never reports -- ProcessFIT
    # overrides it with the message's), and the message names themselves.
    groups = {table["groups"][1] for name, table in ledger["tables"].items() if name not in ("Common", "Dev")}
    groups |= {message["name"] for message in ledger["messages"]}
    args.out.mkdir(parents=True)
    if args.oxidex:
        binary = instrument.resolve_binary(str(args.oxidex))
    else:
        build = subprocess.run(["cargo", "build", "--release", "--bin", "oxidex", "--message-format=json"],
                               cwd=ROOT, text=True, capture_output=True)
        (args.out / "build.jsonl").write_text(build.stdout)
        (args.out / "build.stderr").write_text(build.stderr)
        build.check_returncode()
        paths = [row["executable"] for line in build.stdout.splitlines()
                 if (row := json.loads(line)).get("reason") == "compiler-artifact"
                 and row.get("target", {}).get("name") == "oxidex" and row.get("executable")]
        if len(paths) != 1:
            raise SystemExit("Cargo did not report exactly one oxidex executable")
        binary = instrument.resolve_binary(paths[0])
    fixtures = cases()
    fixtures["Garmin.fit"] = (args.exiftool_dir / "t/images/Garmin.fit").read_bytes()
    instrument.print_header(tool="verify_garmin_fit_reader.py", git=state, binary=binary, oracle=oracle,
                            dirty_overridden=overridden, corpus_paths=[args.out], file_count=len(fixtures))
    rows = []
    failures = []
    for name, contents in sorted(fixtures.items()):
        path = args.out / name
        path.write_bytes(contents)
        for mode, native_flags, oxidex_flags in (("print", [], []), ("no-print-conv", ["-n"], ["--no-print-conv"])):
            native = subprocess.run(oracle.command(["-config", "", "-j", "-a", "-G1", *native_flags, str(path)]),
                                    text=True, capture_output=True, check=True)
            actual = subprocess.run([str(binary.path), "-j", "-a", "-G1", *oxidex_flags, str(path)],
                                    text=True, capture_output=True, check=True)
            prefix = f"{name}.{mode}"
            (args.out / f"{prefix}.native.json").write_text(native.stdout)
            (args.out / f"{prefix}.oxidex.json").write_text(actual.stdout)
            expected, got = project(native.stdout, groups), project(actual.stdout, groups)
            matched = sorted(key for key in expected if got.get(key) == expected[key])
            missing = sorted(key for key in expected if key not in got)
            value = sorted(key for key in expected if key in got and got[key] != expected[key])
            extra = sorted(set(key for key in got if key not in expected)
                           | set(stray_groups(actual.stdout, native.stdout, groups)))
            allowed = EXPECTED_MISSING.get(name, {})
            unexplained = [key for key in missing if key not in allowed]
            # A fixture neither tool parses would pass vacuously.
            substantive = [key for key in matched
                           if key != "File:ProtocolVersion" and not key.startswith("ExifTool:")]
            warnings = {"native": native_warnings(oracle, path), "oxidex": oxidex_warnings(actual.stdout)}
            if warnings["native"] == warnings["oxidex"] and warnings["native"]:
                matched = sorted(matched + ["ExifTool:Warning"])
            if value or extra or unexplained or not substantive or warnings["native"] != warnings["oxidex"]:
                failures.append({"fixture": name, "mode": mode, "value": value, "extra": extra,
                                 "missing_unexplained": unexplained,
                                 "no_substantive_match": not substantive, "warnings": warnings})
            rows.append({"fixture": name, "mode": mode,
                         "fixture_sha256": hashlib.sha256(contents).hexdigest(),
                         "native_sha256": hashlib.sha256(native.stdout.encode()).hexdigest(),
                         "oxidex_sha256": hashlib.sha256(actual.stdout.encode()).hexdigest(),
                         "matched": matched, "missing": missing, "value_mismatch": value, "extra": extra,
                         "warnings": warnings,
                         "expected": expected, "actual": got})
    identities = sorted({key for row in rows for key in row["matched"]})
    occurrences = sum(len(row["matched"]) for row in rows if row["mode"] == "print")
    report = {
        "schema": SCHEMA,
        "instrument": "verify_garmin_fit_reader.py; native and oxidex -j -a -G1 (print and -n); FIT projection",
        "instrument_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "source_commit": state.commit, "source_dirty": state.dirty, "dirty_overridden": overridden,
        "binary": str(binary.path), "binary_sha256": hashlib.sha256(Path(binary.path).read_bytes()).hexdigest(),
        "pin": oracle.version, "oracle": str(args.exiftool_dir), "tz": os.environ.get("TZ"),
        "ledger_sha256": hashlib.sha256(LEDGER.read_bytes()).hexdigest(),
        "fixture_count": len(fixtures), "comparisons": len(rows), "rows": rows,
        "observed_group1_identities": identities,
        "counts": {"distinct_group1_identities_matched": len(identities),
                   "print_mode_matched_occurrences": occurrences,
                   "comparisons_without_failure": len(rows) - len({(f["fixture"], f["mode"]) for f in failures})},
        "failures": failures,
        "writing_observed": None,
        "scope": "default-option FIT behavior fixtures plus t/images/Garmin.fit; not every source row",
    }
    (args.out / "comparison.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report["counts"], indent=2))
    for failure in failures:
        print("FAIL", json.dumps(failure))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
