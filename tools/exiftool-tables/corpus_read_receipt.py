#!/usr/bin/env python3
"""Authenticated corpus read receipt: public OxiDex reads against pinned ExifTool.

Three explicit steps, each refusing a dirty checkout:

  build    Cargo-build the public CLI and record a replayable build proof
           (Cargo stdout/stderr bytes, the identified executable and its hash).
  observe  For every corpus file, record raw transcripts of pinned ExifTool
           (`-j -a -G1:4 -s`, and `-n`), of the proven OxiDex binary (`-j -a
           -G1`, and `--no-print-conv`), and of capture_corpus_sources.pl, which
           names the exact table row behind every native tag.
  verify   Replay a receipt against the repository pin: transcript and stderr
           hashes, command shapes, the build proof, the ExifTool version, DOCX
           capability and canonical Perl, and every derived count.

Metric C (ORIGINAL-OBJECTIVE "Observed behavior") is derived per corpus file
from exact `Group1:TagName` identities. Native family-4 `CopyN` segments and
OxiDex ` (N)` duplicate suffixes are folded, so each identity carries the
multiset of its values. JSON with a duplicate key is refused. Values compare as
parsed JSON with numbers kept as their literal text; there is no loose
normalization. An identity matches in a file only when both modes carry
identical value multisets. Default-mode (print-converted) matches are reported
separately and never credited alone.

Source coordinates are credited exactly. For each file, the native source
capture names the `(table, tag ID, variant index)` behind every occurrence of
an identity; its occurrence count must equal the native JSON value count, or
the identity is unattributable in that file. A coordinate is credited only when
every file in which ExifTool read it matched that identity; one failure
anywhere withholds it.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import instrument  # noqa: E402
import runtime_evidence_inputs as runtime_inputs  # noqa: E402
from native_write_matrix import clean_env  # noqa: E402
from verify_quicktime_userdata_reader import cargo_artifact, check_file, file_fact, sha  # noqa: E402

SCHEMA = "oxidex_corpus_read_receipt_v2"
BUILD_SCHEMA = "oxidex_corpus_read_cli_build_proof_v2"
BUILD_COMMAND = ["cargo", "build", "--release", "--bin", "oxidex", "--message-format=json", "--jobs", "4"]
CANONICAL_PERL = "v5.38.2"
MODES = ("print", "raw")
SOURCE_CAPTURE = HERE / "capture_corpus_sources.pl"
# Filesystem facts differ by construction; they say nothing about parsing.
IGNORED_GROUPS = frozenset({"System"})
IGNORED_KEYS = frozenset({"SourceFile", "ExifTool:ExifToolVersion"})
COPY_SEGMENT = re.compile(r"Copy\d+")
DUPLICATE_SUFFIX = re.compile(r" \(\d+\)$")
TIMEOUT_SECONDS = 120
COUNT_KEYS = ("public_failed_file_modes", "native_file_identities", "extra_file_identities",
              "native_identity_occurrences", "print_mode_matched_file_identities",
              "print_mode_matched_identity_occurrences", "matched_file_identities", "matched_identity_occurrences",
              "missing_file_identities", "mismatched_file_identities", "unattributable_file_identities",
              "credited_source_coordinates", "withheld_source_coordinates")


def pin(root=ROOT) -> str:
    return (root / ".exiftool-version").read_text().strip()


def tool_inputs(root=ROOT):
    names = ("corpus_read_receipt.py", "capture_corpus_sources.pl", "runtime_evidence_inputs.py",
             "native_write_matrix.py", "verify_quicktime_userdata_reader.py")
    return {name: file_fact(root / "tools/exiftool-tables" / name)["sha256"] for name in names}


def clean_snapshot(root=ROOT):
    import quicktime_baseline as baseline
    state = instrument.git_state(root)
    if state.dirty or not state.commit:
        raise ValueError("corpus read evidence requires a clean committed checkout")
    return {"source_commit": state.commit, "source_dirty": False,
            "source_fingerprint": baseline.source_fingerprint(root),
            "runtime_input_manifest_sha256": runtime_inputs.runtime_input_manifest(root),
            "instrument_inputs": tool_inputs(root)}


def build(output: Path, root=ROOT) -> dict:
    output = output.resolve()
    if output.is_relative_to(root.resolve()) or output.exists():
        raise ValueError("choose a new build evidence directory outside the checkout")
    before = clean_snapshot(root)
    output.mkdir(parents=True)
    with (output / "cargo.jsonl").open("wb") as stdout, (output / "cargo.stderr").open("wb") as stderr:
        run = subprocess.run(BUILD_COMMAND, cwd=root, stdout=stdout, stderr=stderr)
    if run.returncode != 0:
        raise ValueError("Cargo CLI build failed; transcripts retained")
    raw_stdout, raw_stderr = (output / "cargo.jsonl").read_bytes(), (output / "cargo.stderr").read_bytes()
    artifact = cargo_artifact(raw_stdout, root)
    proof = {"schema": BUILD_SCHEMA, "snapshot": before, "source_root": str(root.resolve()),
             "command": BUILD_COMMAND, "returncode": run.returncode, "cargo_artifact": artifact,
             "binary": file_fact(artifact["executable"]),
             "cargo_stdout_hex": raw_stdout.hex(), "cargo_stdout_sha256": sha(raw_stdout),
             "cargo_stderr_hex": raw_stderr.hex(), "cargo_stderr_sha256": sha(raw_stderr)}
    if clean_snapshot(root) != before:
        raise ValueError("source changed during Cargo CLI build")
    validate_build_proof(proof, before)
    (output / "build-proof.json").write_text(json.dumps(proof, sort_keys=True, indent=2) + "\n")
    return proof


def validate_build_proof(proof: dict, expected_snapshot: dict) -> None:
    """Replay the Cargo transcript; the binary must be the executable it names."""
    if (not isinstance(proof, dict) or proof.get("schema") != BUILD_SCHEMA or proof.get("command") != BUILD_COMMAND
            or proof.get("returncode") != 0 or proof.get("snapshot") != expected_snapshot
            or not isinstance(expected_snapshot, dict) or expected_snapshot.get("source_dirty") is not False):
        raise ValueError("build proof is malformed, failed, dirty, or belongs to different source inputs")
    for field, width in (("source_commit", 40), ("source_fingerprint", 64), ("runtime_input_manifest_sha256", 64)):
        if not re.fullmatch(r"[0-9a-f]{%d}" % width, str(expected_snapshot.get(field, ""))):
            raise ValueError("build snapshot identity is malformed")
    raw = {}
    for stream in ("stdout", "stderr"):
        try:
            raw[stream] = bytes.fromhex(proof[f"cargo_{stream}_hex"])
        except (KeyError, TypeError, ValueError) as error:
            raise ValueError("build transcript bytes are absent or malformed") from error
        if sha(raw[stream]) != proof.get(f"cargo_{stream}_sha256"):
            raise ValueError("build transcript hash differs")
    source_root = Path(proof.get("source_root", ""))
    if not source_root.is_absolute():
        raise ValueError("build source checkout is missing")
    artifact = cargo_artifact(raw["stdout"], source_root)
    fact = proof.get("binary")
    if (artifact != proof.get("cargo_artifact") or not isinstance(fact, dict) or set(fact) != {"path", "sha256"}
            or Path(artifact["executable"]).resolve() != Path(fact["path"])
            or not re.fullmatch(r"[0-9a-f]{64}", str(fact["sha256"]))):
        raise ValueError("build proof binary differs from the Cargo transcript")


def native_command(native: dict, mode: str, path: str) -> list[str]:
    return [native["perl"]["path"], "-I" + native["library"], native["script"]["path"],
            "-config", "", "-j", "-a", "-G1:4", "-s", *(["-n"] if mode == "raw" else []), path]


def source_command(native: dict, paths: list[str]) -> list[str]:
    return [native["perl"]["path"], native["source_capture"]["path"], native["library"], *paths]


def public_command(proof: dict, mode: str, path: str) -> list[str]:
    return [proof["binary"]["path"], "-j", "-a", "-G1", *(["--no-print-conv"] if mode == "raw" else []), path]


def library_fingerprint(library: Path) -> dict[str, str]:
    return {str(path.relative_to(library)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(library.rglob("*")) if path.is_file() and path.suffix in {".pm", ".pl"}}


def version_commands(facts: dict) -> dict[str, list[str]]:
    perl, library, script = facts["perl"]["path"], facts["library"], facts["script"]["path"]
    return {"version": [perl, "-I" + library, script, "-config", "", "-ver"],
            "capability": [perl, "-I" + library, script, "-config", "", "-s3", "-FileType", facts["capability_file"]],
            "perl": [perl, "-e", "print $^V"]}


def native_identity(perl: Path, tree: Path, expected: str) -> dict:
    library = (tree / "lib").resolve()
    facts = {"perl": file_fact(perl), "script": file_fact(tree / "exiftool"), "library": str(library),
             "library_fingerprint": library_fingerprint(library), "exiftool_version": expected,
             "source_capture": file_fact(SOURCE_CAPTURE),
             "capability_file": str((tree / "t/images/OOXML.docx").resolve())}
    env = clean_env()
    for name, command in version_commands(facts).items():
        facts[name + "_transcript"] = transcript(command, env=env)
    validate_native(facts, expected)
    return facts


def validate_native(facts: dict, expected: str) -> None:
    commands = version_commands(facts)
    version = stdout_of(facts.get("version_transcript"), commands["version"], require_success=True).decode().strip()
    capability = stdout_of(facts.get("capability_transcript"), commands["capability"],
                           require_success=True).decode().strip()
    perl = stdout_of(facts.get("perl_transcript"), commands["perl"], require_success=True).decode()
    if (version != expected or facts.get("exiftool_version") != expected or capability != "DOCX"
            or perl != CANONICAL_PERL):
        raise ValueError("native ExifTool version, DOCX capability or canonical Perl differs from the pin")
    library = Path(facts["library"])
    if Path(facts["script"]["path"]) != (library.parent / "exiftool").resolve():
        raise ValueError("native script is outside the selected ExifTool tree")
    fingerprint = facts.get("library_fingerprint")
    if not isinstance(fingerprint, dict) or not fingerprint or any(
            not re.fullmatch(r"[0-9a-f]{64}", str(value)) for value in fingerprint.values()):
        raise ValueError("native library fingerprint is malformed")


def transcript(command, *, env):
    """Raw process transcript; a non-zero exit or timeout (-1) is recorded, not raised."""
    try:
        run = subprocess.run(command, capture_output=True, env=env, timeout=TIMEOUT_SECONDS)
        returncode, stdout, stderr = run.returncode, run.stdout, run.stderr
    except subprocess.TimeoutExpired as expired:
        returncode, stdout, stderr = -1, expired.stdout or b"", expired.stderr or b""
    return {"command": command, "returncode": returncode,
            **{key + suffix: value for key, raw in (("stdout", stdout), ("stderr", stderr))
               for suffix, value in (("_hex", raw.hex()), ("_sha256", sha(raw)))}}


def stdout_of(fact, command, *, require_success=False) -> bytes:
    """Authenticated stdout of a recorded run; stderr bytes are authenticated too."""
    if (not isinstance(fact, dict) or fact.get("command") != command
            or type(fact.get("returncode")) is not int):
        raise ValueError("transcript command differs or is malformed")
    if require_success and fact["returncode"] != 0:
        raise ValueError("required process failed")
    streams = {}
    for stream in ("stdout", "stderr"):
        try:
            streams[stream] = bytes.fromhex(fact[stream + "_hex"])
        except (KeyError, TypeError, ValueError) as error:
            raise ValueError("malformed transcript bytes") from error
        if sha(streams[stream]) != fact.get(stream + "_sha256"):
            raise ValueError("transcript hash differs")
    return streams["stdout"]


def literal_value(raw: bytes):
    def pairs(items):
        keys = [key for key, _ in items]
        if len(keys) != len(set(keys)):
            raise ValueError("duplicate JSON key")
        return items
    return json.loads(raw, object_pairs_hook=pairs, parse_float=lambda text: {"float": text},
                      parse_int=lambda text: {"int": text})


def canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def identities(raw: bytes, side: str) -> dict[tuple[str, str], list[str]]:
    """-> {(Group1, TagName): sorted canonical values} for one metadata object."""
    document = literal_value(raw)
    if not isinstance(document, list) or len(document) != 1 or not isinstance(document[0], list):
        raise ValueError("expected exactly one metadata JSON object")
    result = defaultdict(list)
    for key, value in document[0]:
        if key in IGNORED_KEYS or ":" not in key:
            continue
        if side == "native":
            segments = key.split(":")
            if len(segments) == 3 and COPY_SEGMENT.fullmatch(segments[1]):
                segments = [segments[0], segments[2]]
            if len(segments) != 2:
                raise ValueError(f"native key has an unexpected group shape: {key}")
            group, name = segments
        else:
            group, name = key.split(":", 1)
            name = DUPLICATE_SUFFIX.sub("", name)
        if group in IGNORED_GROUPS:
            continue
        result[(group, name)].append(canonical(value))
    return {identity: sorted(values) for identity, values in result.items()}


def source_rows(receipt: dict) -> dict[str, dict[tuple[str, str], Counter]]:
    """-> {file: {(Group1, TagName): Counter of (table, tag ID, variant) or None}}."""
    native, root = receipt["native"], Path(receipt["corpus"]["root"])
    names = list(receipt["corpus"]["files"])
    raw = stdout_of(receipt["sources"], source_command(native, [str(root / name) for name in names]),
                    require_success=True)
    document = json.loads(raw)
    if not isinstance(document, dict) or set(document) != {str(root / name) for name in names}:
        raise ValueError("source capture does not cover exactly the corpus")
    result = {}
    for name in names:
        by_identity = defaultdict(Counter)
        for row in document[str(root / name)]:
            if not isinstance(row, list) or len(row) != 5:
                raise ValueError("source capture row is malformed")
            group, tag, table, tag_id, variant = row
            coordinate = (table, tag_id, variant) if variant is not None else None
            by_identity[(group, tag)][coordinate] += 1
        result[name] = by_identity
    return result


def derive(receipt: dict) -> dict:
    """Recompute every count and credit from the raw transcripts in the receipt."""
    proof, native = receipt["build_proof"], receipt["native"]
    files = receipt["corpus"]["files"]
    seen = set()
    for row in receipt["observations"]:
        pair = (row["file"], row["mode"])
        if row["file"] not in files or row["mode"] not in MODES or pair in seen:
            raise ValueError("observation grid is unknown or duplicated")
        seen.add(pair)
    if seen != {(name, mode) for name in files for mode in MODES}:
        raise ValueError("observation grid is incomplete")
    # Every count is present, zero included, so a receipt's shape never varies.
    counts = Counter({key: 0 for key in COUNT_KEYS})
    by_file = defaultdict(dict)
    for row in receipt["observations"]:
        path = str(Path(receipt["corpus"]["root"]) / row["file"])
        # Native output must succeed and parse: a native failure would
        # silently hide every tag it should have reported.
        expected = identities(stdout_of(row["native"], native_command(native, row["mode"], path),
                                        require_success=True), "native")
        public_fact = row["oxidex"]
        public_raw = stdout_of(public_fact, public_command(proof, row["mode"], path))
        try:
            if public_fact["returncode"] != 0:
                raise ValueError("public read failed")
            actual = identities(public_raw, "public")
        except ValueError:
            # A failed or unparseable public read leaves every native tag missing.
            actual = {}
            counts["public_failed_file_modes"] += 1
        by_file[row["file"]][row["mode"]] = (expected, actual)
    sources = source_rows(receipt)
    per_identity_files = defaultdict(set)
    print_identities, native_identities = set(), set()
    coordinate_files = defaultdict(lambda: {"matched": set(), "failed": set()})
    for name, modes in sorted(by_file.items()):
        (expected_print, actual_print), (expected_raw, actual_raw) = modes["print"], modes["raw"]
        native_identities.update(f"{g}:{n}" for g, n in expected_print)
        counts["native_file_identities"] += len(expected_print)
        counts["extra_file_identities"] += sum(1 for identity in actual_print if identity not in expected_print)
        for identity, values in expected_print.items():
            counts["native_identity_occurrences"] += len(values)
            print_match = actual_print.get(identity) == values
            matched = (print_match and expected_raw.get(identity) is not None
                       and actual_raw.get(identity) == expected_raw[identity])
            if print_match:
                counts["print_mode_matched_file_identities"] += 1
                counts["print_mode_matched_identity_occurrences"] += len(values)
                print_identities.add(f"{identity[0]}:{identity[1]}")
            if matched:
                counts["matched_file_identities"] += 1
                counts["matched_identity_occurrences"] += len(values)
                per_identity_files[f"{identity[0]}:{identity[1]}"].add(name)
            elif identity not in actual_print:
                counts["missing_file_identities"] += 1
            else:
                counts["mismatched_file_identities"] += 1
            coordinates = sources[name].get(identity, Counter())
            if sum(coordinates.values()) != len(values) or None in coordinates:
                counts["unattributable_file_identities"] += 1
                continue
            for coordinate in coordinates:
                coordinate_files[coordinate]["matched" if matched else "failed"].add(name)
    credited = sorted(list(coordinate) for coordinate, result in coordinate_files.items()
                      if result["matched"] and not result["failed"])
    counts["credited_source_coordinates"] = len(credited)
    counts["withheld_source_coordinates"] = sum(1 for result in coordinate_files.values()
                                                if result["matched"] and result["failed"])
    return {"matched_identities": {identity: sorted(names) for identity, names in sorted(per_identity_files.items())},
            "credited_coordinates": credited,
            "metric_c": {"corpus_files": len(files),
                         "distinct_group1_tag_identities_native": len(native_identities),
                         "distinct_group1_tag_identities_matched": len(per_identity_files),
                         "distinct_group1_tag_identities_print_mode_matched": len(print_identities),
                         **dict(sorted(counts.items()))}}


def observe(args) -> Path:
    output = args.output.resolve()
    if output.exists() or output.is_relative_to(ROOT.resolve()):
        raise ValueError("choose a new evidence directory outside the checkout")
    proof = json.loads(args.build_proof.read_bytes())
    snapshot = clean_snapshot()
    validate_build_proof(proof, snapshot)
    check_file(proof["binary"])
    expected = pin()
    native = native_identity(args.perl.resolve(), args.exiftool_dir.resolve(), expected)
    corpus = args.corpus.resolve()
    files = {str(path.relative_to(corpus)): file_fact(path)["sha256"]
             for path in sorted(corpus.rglob("*")) if path.is_file() and not path.name.startswith(".")}
    if len(files) < args.min_files:
        raise ValueError(f"corpus has {len(files)} files; floor is {args.min_files}")
    instrument.print_header(tool="corpus_read_receipt.py", git=instrument.git_state(ROOT),
                            extra=[f"oxidex: {proof['binary']['path']} ({proof['binary']['sha256'][:12]})",
                                   f"native: ExifTool {expected} via {native['perl']['path']}",
                                   f"corpus: {corpus} ({len(files)} files)"])
    output.mkdir(parents=True)
    env = clean_env()
    receipt = {"schema": SCHEMA, "instrument": "corpus_read_receipt.py", "producer": snapshot,
               "build_proof": proof, "native": native,
               "corpus": {"root": str(corpus), "files": files}, "observations": [],
               "sources": transcript(source_command(native, [str(corpus / name) for name in files]), env=env),
               "scope": "public CLI reads of every corpus file in print and raw modes; "
                        "exact source-coordinate credit; no writes"}
    for name in files:
        path = str(corpus / name)
        for mode in MODES:
            receipt["observations"].append({
                "file": name, "mode": mode,
                "native": transcript(native_command(native, mode, path), env=env),
                "oxidex": transcript(public_command(proof, mode, path), env=env)})
        with (output / "progress.jsonl").open("a") as progress:
            progress.write(json.dumps({"file": name}) + "\n")
    if clean_snapshot() != snapshot or {name: file_fact(corpus / name)["sha256"] for name in files} != files:
        raise ValueError("source or corpus changed during observation")
    receipt.update(derive(receipt))
    target = output / "receipt.json"
    target.write_text(json.dumps(receipt, sort_keys=True) + "\n")
    print(json.dumps(receipt["metric_c"], indent=2, sort_keys=True))
    return target


def validate(receipt: dict, expected_version: str) -> dict:
    """Integrity of a stored receipt, replayed from its own bytes against the repository pin."""
    if receipt.get("schema") != SCHEMA:
        raise ValueError("corpus read receipt schema differs")
    validate_build_proof(receipt.get("build_proof"), receipt.get("producer"))
    validate_native(receipt["native"], expected_version)
    files = receipt.get("corpus", {}).get("files")
    if not isinstance(files, dict) or not files or any(
            not re.fullmatch(r"[0-9a-f]{64}", str(value)) for value in files.values()):
        raise ValueError("corpus file identities are malformed")
    derived = derive(receipt)
    for key, value in derived.items():
        if receipt.get(key) != value:
            raise ValueError(f"derived claim differs from transcripts: {key}")
    return derived


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    build_parser = sub.add_parser("build")
    build_parser.add_argument("--output", type=Path, required=True)
    observe_parser = sub.add_parser("observe")
    observe_parser.add_argument("--build-proof", type=Path, required=True)
    observe_parser.add_argument("--perl", type=Path, required=True)
    observe_parser.add_argument("--exiftool-dir", type=Path, required=True)
    observe_parser.add_argument("--corpus", type=Path, required=True)
    observe_parser.add_argument("--output", type=Path, required=True)
    observe_parser.add_argument("--min-files", type=int, default=1)
    verify_parser = sub.add_parser("verify")
    verify_parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "build":
        print(json.dumps(build(args.output)["binary"], indent=2))
    elif args.command == "observe":
        print(observe(args))
    else:
        derived = validate(json.loads(args.receipt.read_bytes()), pin())
        print("=== instrument: corpus_read_receipt.py verify ===")
        print(json.dumps(derived["metric_c"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f"corpus read receipt refused: {exc}")
