#!/usr/bin/env python3
"""Concrete, non-promoting commands for a version-rehearsal stage.

This is deliberately an adapter, not the scheduler.  The scheduler owns the
selected releases and invokes this program once per stage.  Every mutation is
limited to that release's owned checkout and target directory.  In particular,
the adapter never falls back to the repository's normal ExifTool pin.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
from typing import Any, Callable

import artifacts
import generated_tiff_write_matrix as generated_matrix
import native_write_matrix as native
import version_rehearsal as rehearsal
import version_rehearsal_executor as executor

RELEASE = re.compile(r"^[0-9]+\.[0-9]+$")
OID = rehearsal.GIT_OID_RE
COMMAND_TIMEOUT_SECONDS = 3600
READ_FIXTURE_KIND = "oxidex_version_rehearsal_fixture_manifest"
WRITE_FIXTURE_KIND = "oxidex_version_rehearsal_write_fixture_manifest"

# The v4 matrix exposes these source-derived operands.  Keep the adapters as
# module globals so the offline stage tests can supply a complete fixture
# contract without invoking native generation.
GeneratedTarget = generated_matrix.GeneratedTarget
generated_targets = generated_matrix.generated_targets
case_inputs = getattr(generated_matrix, "case_inputs", None)
explicit_directory_operands = getattr(generated_matrix, "explicit_directory_operands", None)
selected_qualifiers = getattr(generated_matrix, "selected_qualifiers", None)
directory_path = getattr(generated_matrix, "directory_path", None)
matrix_inputs = getattr(generated_matrix, "matrix_inputs", None)
public_scalar = getattr(generated_matrix, "public_scalar", None)


class Refused(ValueError):
    pass


def _sha(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def _regular(path: Path, label: str) -> Path:
    if path.is_symlink() or not path.is_file():
        raise Refused(f"{label} must be a regular file")
    return path.resolve()


def _directory(path: Path, label: str) -> Path:
    if path.is_symlink() or not path.is_dir():
        raise Refused(f"{label} must be a directory")
    return path.resolve()


def _json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(_regular(path, "JSON input").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise Refused("JSON input is unreadable") from exc
    if not isinstance(value, dict):
        raise Refused("JSON input must be an object")
    return value


def _atomic(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    if temporary.exists() or temporary.is_symlink():
        raise Refused("stage temporary report already exists")
    temporary.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def _run(argv: list[str], *, cwd: Path, env: dict[str, str], run: Callable[..., subprocess.CompletedProcess[str]]) -> dict[str, Any]:
    """Run below the executor's process group and retain the actual output.

    The executor owns a session and an inheritable host-lock descriptor.  A
    nested generation/build/comparison process must stay in that group so an
    executor timeout kills it too and its inherited lock cannot outlive the
    supervisor.  Do not create another session here.
    """
    try:
        if run is subprocess.run:
            child = subprocess.Popen(argv, cwd=str(cwd), env=env, text=True, errors="replace", stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, start_new_session=False, close_fds=False)
            try:
                stdout, stderr = child.communicate(timeout=COMMAND_TIMEOUT_SECONDS)
            except subprocess.TimeoutExpired as exc:
                child.terminate()
                try: stdout, stderr = child.communicate(timeout=10)
                except subprocess.TimeoutExpired:
                    child.kill(); stdout, stderr = child.communicate()
                return {"argv": argv, "cwd": str(cwd), "exit": None, "state": "timeout", "stdout": stdout or "", "stderr": (stderr or "") + str(exc), "pid": child.pid, "pgid": os.getpgid(child.pid) if child.poll() is None else None}
            completed = subprocess.CompletedProcess(argv, child.returncode, stdout, stderr)
            process = {"pid": child.pid, "pgid": os.getpgid(child.pid) if child.poll() is None else None}
        else:
            completed = run(argv, cwd=str(cwd), env=env, text=True, errors="replace", capture_output=True,
                            timeout=COMMAND_TIMEOUT_SECONDS, start_new_session=False, close_fds=False)
            process = {}
        stdout, stderr = completed.stdout or "", completed.stderr or ""
        return {"argv": argv, "cwd": str(cwd), "exit": completed.returncode, "state": "ok" if completed.returncode == 0 else "exit_failed", "stdout": stdout, "stderr": stderr, **process}
    except subprocess.TimeoutExpired as exc:
        return {"argv": argv, "cwd": str(cwd), "exit": None, "state": "timeout", "stdout": "", "stderr": str(exc)}
    except OSError as exc:
        return {"argv": argv, "cwd": str(cwd), "exit": None, "state": "spawn_failed", "stdout": "", "stderr": str(exc)}


def _raw(report: Path, stage: str, record: dict[str, Any]) -> dict[str, str]:
    path = report.parent / "raw" / f"{stage}-command.json"
    _atomic(path, record)
    return {"path": str(path), "sha256": _sha(path)}


def _git(checkout: Path, args: list[str], run: Callable[..., subprocess.CompletedProcess[str]]) -> str:
    record = _run(["git", "-C", str(checkout), *args], cwd=checkout, env=dict(os.environ), run=run)
    if record["state"] != "ok":
        raise Refused("cannot inspect owned checkout")
    return record["stdout"].strip()


def _native_identity(release: str, perl: Path, native_source: Path, native_lib: Path) -> dict[str, Any]:
    pm = _regular(native_lib / "Image" / "ExifTool.pm", "selected native library")
    text = pm.read_text(encoding="utf-8", errors="replace")
    match = re.search(r"^\s*\$VERSION\s*=\s*['\"]([^'\"]+)['\"]", text, re.MULTILINE)
    if match is None or match.group(1) != release:
        raise Refused("selected native library does not match selected release")
    return {"release": release, "perl": {"path": str(perl), "sha256": _sha(perl)},
            "source": {"path": str(native_source)}, "lib": {"path": str(native_lib), "exiftool_pm_sha256": _sha(pm)}}


def _native_writer_sources(native_lib: Path) -> dict[str, str]:
    """Bind the matrix's executable native identity to selected source bytes."""
    paths = {
        "Image/ExifTool.pm": native_lib / "Image" / "ExifTool.pm",
        "Image/ExifTool/Writer.pl": native_lib / "Image" / "ExifTool" / "Writer.pl",
    }
    return {name: _sha(_regular(path, f"selected native {name}")) for name, path in paths.items()}


def _source_tree(checkout: Path) -> str:
    """Hash every source entry, including untracked entries, excluding caches."""
    entries: dict[str, Any] = {}
    for directory, dirs, names in os.walk(checkout, followlinks=False):
        root = Path(directory)
        dirs[:] = sorted(d for d in dirs if d not in {".git", "target", "__pycache__"})
        links = [d for d in dirs if (root / d).is_symlink()]
        dirs[:] = [d for d in dirs if d not in links]
        for name in sorted(name for name in names + links if name != ".git"):
            path = root / name
            relative = path.relative_to(checkout).as_posix()
            if path.is_symlink():
                entries[relative] = ["symlink", os.readlink(path)]
            elif path.is_file():
                entries[relative] = ["file", _sha(path)]
            else:
                raise Refused("owned checkout contains unsupported source entry")
    return rehearsal.sha256_json(entries)


def _artifact_rows(checkout: Path) -> list[dict[str, Any]]:
    rows = []
    for item in artifacts.inventory(checkout):
        path = _regular(checkout / item.path, f"generated artifact {item.path}")
        rows.append({"path": item.path, "sha256": _sha(path), "bytes": path.stat().st_size})
    return rows


def _validate_artifacts(checkout: Path, rows: Any) -> list[dict[str, Any]]:
    expected = [item.path for item in artifacts.inventory(checkout)]
    if not isinstance(rows, list) or len(rows) != len(expected):
        raise Refused("generated artifact proof is incomplete")
    found: list[str] = []
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get("path"), str) or row["path"] not in expected:
            raise Refused("generated artifact path is invalid")
        if type(row.get("bytes")) is not int or row["bytes"] < 0 or not isinstance(row.get("sha256"), str) or re.fullmatch(r"[0-9a-f]{64}", row["sha256"]) is None:
            raise Refused("generated artifact identity is invalid")
        path = _regular(checkout / PurePosixPath(row["path"]), "generated artifact")
        if _sha(path) != row["sha256"] or path.stat().st_size != row["bytes"]:
            raise Refused("generated artifact no longer matches its proof")
        found.append(row["path"])
    if found != expected:
        raise Refused("generated artifact order or identity differs from sanctioned manifest")
    return rows


def generated_refusal_counts(checkout: Path) -> dict[str, Any]:
    """Count explicit refusal/omission fields in the live generated ledgers.

    This is evidence that a historical generation refused source behavior
    explicitly; it is not permission to reinterpret a refusal as coverage.
    JSON paths are retained so a total can never hide which generated ledger
    and field contributed to it.
    """
    counters: list[dict[str, Any]] = []
    key_pattern = re.compile(r"(?:refus|omitt|withheld)", re.IGNORECASE)

    def walk(value: Any, location: str, artifact_path: str) -> None:
        if isinstance(value, dict):
            for key in sorted(value):
                child = value[key]
                child_location = f"{location}.{key}" if location else key
                if key_pattern.search(key):
                    if type(child) is int and child >= 0:
                        counters.append({"artifact": artifact_path, "json_path": child_location,
                                         "kind": "integer", "count": child})
                    elif isinstance(child, list):
                        counters.append({"artifact": artifact_path, "json_path": child_location,
                                         "kind": "array-length", "count": len(child)})
                walk(child, child_location, artifact_path)
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, f"{location}[{index}]", artifact_path)

    for item in artifacts.inventory(checkout):
        if not item.path.endswith(".json"):
            continue
        artifact_path = _regular(checkout / item.path, f"generated refusal ledger {item.path}")
        try:
            document = json.loads(artifact_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise Refused(f"generated refusal ledger is unreadable: {item.path}") from exc
        walk(document, "", item.path)
    counters.sort(key=lambda row: (row["artifact"], row["json_path"], row["kind"]))
    return {"kind": "explicit-generated-refusal-counts", "counters": counters,
            "total": sum(row["count"] for row in counters)}


def _environment(perl: Path, native_lib: Path, target: Path) -> dict[str, str]:
    env = dict(os.environ)
    for key in ("PERL5LIB", "PERLLIB", "PERL5OPT", "PERL_MM_OPT", "PERL_MB_OPT", "PERL_LOCAL_LIB_ROOT"):
        env.pop(key, None)
    env.update(EXIFTOOL_PERL=str(perl), OXIDEX_EXIFTOOL_LIB=str(native_lib),
               OXIDEX_ET_CACHE=str(target / "exiftool-cache"), CARGO_TARGET_DIR=str(target))
    return env


def _common(args: argparse.Namespace, run: Callable[..., subprocess.CompletedProcess[str]]) -> tuple[Path, Path, Path, Path, Path, Path, dict[str, Any]]:
    if RELEASE.fullmatch(args.release) is None or OID.fullmatch(args.source_commit) is None:
        raise Refused("release or immutable source commit is malformed")
    checkout = _directory(Path(args.checkout), "owned checkout")
    target = _directory(Path(args.target), "isolated target")
    report = Path(args.report).absolute()
    if report.exists() or report.is_symlink():
        raise Refused("stage report already exists")
    perl = _regular(Path(args.native_perl), "selected native Perl")
    if not os.access(perl, os.X_OK):
        raise Refused("selected native Perl is not executable")
    native_source = _directory(Path(args.native_source), "selected native source")
    native_lib = _directory(Path(args.native_lib), "selected native library")
    if not native_lib.is_relative_to(native_source):
        raise Refused("selected native library must belong to selected native source")
    if _git(checkout, ["rev-parse", "HEAD"], run) != args.source_commit:
        raise Refused("owned checkout HEAD differs from immutable execution source")
    identity = _native_identity(args.release, perl, native_source, native_lib)
    return checkout, target, report, perl, native_source, native_lib, identity


def _base(stage: str, args: argparse.Namespace, checkout: Path, identity: dict[str, Any]) -> dict[str, Any]:
    return {"schema": executor.SCHEMA, "kind": executor.RESULT_KIND, "stage": stage, "release": args.release,
            "source_commit": args.source_commit, "source_tree_sha256": _source_tree(checkout), "native_identity": identity}


def generate(args: argparse.Namespace, *, run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run) -> dict[str, Any]:
    checkout, target, report, perl, _source, native_lib, identity = _common(args, run)
    if _git(checkout, ["status", "--porcelain=v1"], run) != "":
        raise Refused("owned checkout must be clean before sanctioned generation")
    clean_source = _source_tree(checkout)
    pin = checkout / ".exiftool-version"
    _regular(pin, "owned checkout pin")
    pin.write_text(args.release + "\n", encoding="utf-8")
    if _git(checkout, ["diff", "--name-only"], run) not in {"", ".exiftool-version"}:
        raise Refused("only the owned checkout pin may change before generation")
    env = _environment(perl, native_lib, target)
    # Selection changes only this owned checkout's pin before the sanctioned
    # generator runs.  Its Tier 1 expression verification invokes the shared
    # measurement guard, which otherwise rejects that required, attributable
    # pin diff before generation can reach its artifact proof.
    env["OXIDEX_ALLOW_DIRTY_TREE"] = "1"
    record = _run(["bash", str(checkout / "tools" / "exiftool-tables" / "regen-all.sh")], cwd=checkout, env=env, run=run)
    raw = _raw(report, "generate", record)
    if record["state"] != "ok":
        raise Refused("sanctioned regen-all.sh failed")
    if pin.read_text(encoding="utf-8") != args.release + "\n":
        raise Refused("sanctioned generation changed selected release pin")
    result = {**_base("generate", args, checkout, identity), "state": "passed", "denominator": 1,
              "generated_artifacts": _artifact_rows(checkout), "raw_report": raw,
              "clean_source_before": {"git_status": "clean", "source_tree_sha256": clean_source},
              "sanctioned_command": "tools/exiftool-tables/regen-all.sh"}
    _atomic(report, result)
    return result


def _prior(report: Path, stage: str, args: argparse.Namespace, identity: dict[str, Any], checkout: Path) -> dict[str, Any]:
    value = _json(report.parent / f"{stage}.json")
    if (value.get("schema") != executor.SCHEMA or value.get("kind") != executor.RESULT_KIND
            or value.get("stage") != stage or value.get("state") != "passed" or value.get("release") != args.release
            or value.get("source_commit") != args.source_commit or value.get("native_identity") != identity
            or value.get("source_tree_sha256") != _source_tree(checkout)):
        raise Refused("prior stage is not bound to this release, source, and native identity")
    return value


def _cargo_executable(record: dict[str, Any], checkout: Path, target: Path,
                      *, test: bool) -> dict[str, Any]:
    """Use Cargo's exact package/target/profile evidence, never a guessed path."""
    candidates: set[Path] = set()
    kind = "lib" if test else "bin"
    for line in record["stdout"].splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(row, dict) or row.get("reason") != "compiler-artifact":
            continue
        item = row.get("target", {})
        if (item.get("name") != "oxidex" or kind not in item.get("kind", [])
                or row.get("profile", {}).get("test") is not test
                or not isinstance(row.get("manifest_path"), str)
                or Path(row["manifest_path"]).resolve() != checkout / "Cargo.toml"):
            continue
        if not isinstance(row.get("executable"), str):
            continue
        candidate = _regular(Path(row["executable"]), "cargo JSON executable")
        if not candidate.is_relative_to(target) or not os.access(candidate, os.X_OK):
            raise Refused("cargo JSON executable is not executable inside the isolated target")
        candidates.add(candidate)
    if len(candidates) != 1:
        raise Refused(f"cargo JSON did not identify one isolated oxidex {kind} executable")
    executable = candidates.pop()
    return {"path": str(executable), "sha256": _sha(executable), "bytes": executable.stat().st_size}


def build(args: argparse.Namespace, *, run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run) -> dict[str, Any]:
    checkout, target, report, perl, _source, native_lib, identity = _common(args, run)
    if (checkout / ".exiftool-version").read_text(encoding="utf-8") != args.release + "\n":
        raise Refused("build checkout is not pinned to selected release")
    generated = _validate_artifacts(checkout, _prior(report, "generate", args, identity, checkout).get("generated_artifacts"))
    env = _environment(perl, native_lib, target)
    records = []
    for command in (["cargo", "build", "--all-features", "--message-format=json", "--bin", "oxidex"],
                    ["cargo", "test", "--lib", "--all-features", "--no-run", "--message-format=json"]):
        record = _run(command, cwd=checkout, env=env, run=run)
        records.append(record)
        if record["state"] != "ok":
            _raw(report, "build", {"commands": records, "state": "failed"})
            raise Refused("cargo build or writer-driver compilation failed")
    raw = _raw(report, "build", {"commands": records, "state": "ok"})
    executable = _cargo_executable(records[0], checkout, target, test=False)
    writer = _cargo_executable(records[1], checkout, target, test=True)
    if executable["path"] == writer["path"]:
        raise Refused("CLI and writer driver must be distinct Cargo executables")
    # A build may not change the generated inputs whose identity it claims.
    _validate_artifacts(checkout, generated)
    result = {**_base("build", args, checkout, identity), "state": "passed", "denominator": 2,
              "generated_artifacts": generated, "binary": executable,
              "writer_binary": writer, "raw_report": raw}
    _atomic(report, result); return result


def _fixtures(manifest: Path, target: Path, *, kind: str, subtarget: str) -> tuple[list[dict[str, Any]], str, Path]:
    source = _json(manifest)
    if source.get("schema") != 1 or source.get("kind") != kind or not isinstance(source.get("fixtures"), list) or not source["fixtures"]:
        raise Refused("fixture manifest schema is unsupported")
    digest = _sha(manifest); corpus = target / subtarget / digest
    if corpus.exists() or corpus.is_symlink(): raise Refused("fixture corpus already exists")
    corpus.mkdir(parents=True)
    rows = []
    for index, item in enumerate(source["fixtures"]):
        if not isinstance(item, dict) or not isinstance(item.get("path"), str) or not isinstance(item.get("sha256"), str) or type(item.get("bytes")) is not int:
            raise Refused("fixture manifest item is malformed")
        original = _regular(Path(item["path"]), "fixture")
        if _sha(original) != item["sha256"] or original.stat().st_size != item["bytes"]: raise Refused("fixture changed after manifest")
        copied = corpus / f"{index:04d}-{hashlib.sha256(str(original).encode()).hexdigest()[:16]}{original.suffix}"
        shutil.copyfile(original, copied); copied.chmod(0o444)
        if _sha(copied) != item["sha256"] or copied.stat().st_size != item["bytes"]:
            raise Refused("staged fixture copy differs from immutable manifest")
        rows.append({"source": str(original), "sha256": item["sha256"], "bytes": item["bytes"], "corpus_path": str(copied), "corpus_sha256": _sha(copied), "corpus_bytes": copied.stat().st_size})
    return rows, digest, corpus


def _write_fixtures(manifest: Path, target: Path) -> tuple[list[dict[str, Any]], str, Path]:
    rows, digest, corpus = _fixtures(manifest, target, kind=WRITE_FIXTURE_KIND,
                                     subtarget="rehearsal-write-fixtures")
    for row in rows:
        if _regular(Path(row["corpus_path"]), "staged JPEG write fixture").read_bytes()[:2] != b"\xff\xd8":
            raise Refused("write fixture manifest contains a non-JPEG fixture")
    return rows, digest, corpus


def _verify_staged_fixtures(rows: list[dict[str, Any]]) -> None:
    for row in rows:
        source = _regular(Path(row["source"]), "fixture source")
        if _sha(source) != row["sha256"] or source.stat().st_size != row["bytes"]:
            raise Refused("fixture source changed during comparison")
        staged = _regular(Path(row["corpus_path"]), "staged fixture")
        if (_sha(staged) != row["sha256"] or staged.stat().st_size != row["bytes"]
                or row.get("corpus_sha256") != row["sha256"] or row.get("corpus_bytes") != row["bytes"]):
            raise Refused("staged fixture changed during read comparison")


def _verify_conformance_scope(data: dict[str, Any], rows: list[dict[str, Any]]) -> None:
    per_file = data.get("per_file")
    expected = {row["corpus_path"] for row in rows}
    if not isinstance(per_file, dict) or set(per_file) != expected:
        raise Refused("conformance report did not cover exactly the staged fixture manifest")
    per_format = data.get("per_format")
    if not isinstance(per_format, dict):
        raise Refused("conformance report lacks per-format counts")
    file_count = 0
    for counts in per_format.values():
        if not isinstance(counts, dict) or type(counts.get("files")) is not int or counts["files"] < 0:
            raise Refused("conformance report has malformed per-format file counts")
        file_count += counts["files"]
    if file_count != len(rows):
        raise Refused("conformance report file count differs from staged fixture manifest")


def read(args: argparse.Namespace, *, run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run) -> dict[str, Any]:
    checkout, target, report, perl, native_source, native_lib, identity = _common(args, run)
    if re.fullmatch(r"[0-9a-f]{64}", args.native_probe_sha256) is None:
        raise Refused("native probe digest is malformed")
    previous = _prior(report, "build", args, identity, checkout)
    generated = _validate_artifacts(checkout, previous.get("generated_artifacts"))
    binary = previous.get("binary")
    if not isinstance(binary, dict) or not isinstance(binary.get("path"), str) or not isinstance(binary.get("sha256"), str): raise Refused("build binary proof is malformed")
    executable = _regular(Path(binary["path"]), "built oxidex executable")
    if _sha(executable) != binary["sha256"] or executable.stat().st_size != binary.get("bytes"): raise Refused("built oxidex executable changed")
    fixtures, fixture_digest, corpus = _fixtures(Path(args.fixture_manifest), target,
                                                  kind=READ_FIXTURE_KIND, subtarget="rehearsal-fixtures")
    comparison = report.parent / "raw" / "read-conformance.json"
    command = [sys.executable, str(checkout / "tools" / "exiftool-tables" / "conformance.py"), str(corpus), "--recursive", "--exiftool-dir", str(native_source), "--oxidex", str(executable), "--min-files", str(len(fixtures)), "--min-tags", "1", "--json-out", str(comparison)]
    env = _environment(perl, native_lib, target); env["OXIDEX_ALLOW_DIRTY_TREE"] = "1"
    record = _run(command, cwd=checkout, env=env, run=run); raw = _raw(report, "read", record)
    if record["state"] != "ok" or not comparison.is_file(): raise Refused("actual conformance.py comparison failed")
    _verify_staged_fixtures(fixtures)
    data = _json(comparison); _verify_conformance_scope(data, fixtures); per_format = data["per_format"]
    classification_counts = {"matched": 0, "value_diff": 0, "missing": 0, "renames": 0, "extra": 0}
    for format_counts in per_format.values():
        if not isinstance(format_counts, dict): raise Refused("conformance format counts are malformed")
        for key in ("matched", "value_diff", "missing", "renames", "extra"):
            if type(format_counts.get(key)) is not int or format_counts[key] < 0: raise Refused("conformance count is malformed")
        for key in ("matched", "value_diff", "missing", "renames", "extra"):
            classification_counts[key] += format_counts[key]
    matched = classification_counts["matched"]
    mismatched = sum(classification_counts[key] for key in ("value_diff", "missing", "renames", "extra"))
    denominator = matched + mismatched
    state = "passed" if denominator > 0 and mismatched == 0 else "failed"
    result = {**_base("read", args, checkout, identity), "state": state, "denominator": denominator,
              "native_release": args.release, "native_probe_sha256": args.native_probe_sha256,
              "comparison": {"kind": "oxidex_vs_native", "native_release": args.release, "matched": matched, "mismatched": mismatched},
              "classification_counts": classification_counts,
              "generated_artifacts": generated, "binary": {"path": str(executable), "sha256": _sha(executable), "bytes": executable.stat().st_size},
              "fixtures": {"manifest": str(Path(args.fixture_manifest).absolute()), "manifest_sha256": fixture_digest, "entries": fixtures}, "raw_report": raw,
              "conformance_report": {"path": str(comparison), "sha256": _sha(comparison)}}
    _atomic(report, result)
    return result


def _build_binary(previous: dict[str, Any], target: Path, field: str, label: str) -> dict[str, Any]:
    row = previous.get(field)
    if (not isinstance(row, dict) or not isinstance(row.get("path"), str)
            or not isinstance(row.get("sha256"), str) or type(row.get("bytes")) is not int):
        raise Refused(f"build {label} proof is malformed")
    binary = _regular(Path(row["path"]), f"built {label}")
    if (not binary.is_relative_to(target.resolve()) or _sha(binary) != row["sha256"]
            or binary.stat().st_size != row["bytes"]):
        raise Refused(f"built {label} changed after build")
    return {"path": str(binary), "sha256": _sha(binary), "bytes": binary.stat().st_size}


def _v4_callable(name: str) -> Callable[..., Any]:
    value = globals().get(name)
    if not callable(value):
        raise Refused(f"generated write matrix v4 operand {name} is unavailable")
    return value


def _matrix_source_artifacts(checkout: Path) -> dict[str, Path]:
    """All generated source inputs that authorize the v4 public matrix."""
    paths = {
        "final_ledger": checkout / "tools/exiftool-tables/tiff_scalar_final_ledger.json",
        "final_rules": checkout / "src/writers/generated_tiff_scalar_final_rules.rs",
        "scalar_helper_ledger": checkout / "tools/exiftool-tables/scalar_helper_ledger.json",
        "scalar_rules": checkout / "src/writers/generated_scalar_rules.rs",
        "address_rules": checkout / "src/writers/generated_setnewvalue_address_rules.rs",
    }
    for label, source in paths.items():
        _regular(source, f"generated write {label.replace('_', ' ')}")
    return paths


def _source_proof(paths: dict[str, Path]) -> dict[str, dict[str, str]]:
    return {label: {"path": str(path), "sha256": _sha(path)} for label, path in paths.items()}


def _matrix_contract(paths: dict[str, Path]) -> tuple[list[dict[str, Any]], tuple[str, ...], set[tuple[Any, ...]], int]:
    """Recompute every v4 row from authenticated generated source operands."""
    try:
        targets: tuple[GeneratedTarget, ...] = generated_targets(paths["final_ledger"], paths["final_rules"])
        directories = tuple(_v4_callable("explicit_directory_operands")(
            rules_path=paths["scalar_rules"], ledger_path=paths["scalar_helper_ledger"], address_path=paths["address_rules"]))
    except (TypeError, ValueError, OSError, KeyError) as exc:
        raise Refused("generated write v4 source cohort is unavailable") from exc
    if directories != ("IFD0", "IFD1"):
        raise Refused("generated write v4 explicit directory operands are incomplete")
    source_cohort, expected, identities = [], set(), set()
    if not targets:
        raise Refused("generated write v4 source cohort is empty")
    original_subset = 0
    for target in targets:
        try:
            target_identity = {"raw_tag_id": target.raw_tag_id, "name": target.name,
                               "table_group0": target.table_group0,
                               "physical_write_group": target.physical_write_group,
                               "wire_format": target.wire_format}
            family = target.case_family
            original = _v4_callable("case_inputs")(target)
            inputs = _v4_callable("matrix_inputs")(target)
            baseline_qualifiers = tuple(target.qualifiers)
            qualifiers = tuple(_v4_callable("selected_qualifiers")(target, directories))
        except (AttributeError, TypeError, ValueError) as exc:
            raise Refused("generated write v4 target contract is malformed") from exc
        if (set(target_identity) != {"raw_tag_id", "name", "table_group0", "physical_write_group", "wire_format"}
                or type(target_identity["raw_tag_id"]) is not int or not 0 <= target_identity["raw_tag_id"] <= 0xffff
                or any(not isinstance(target_identity[name], str) or not target_identity[name]
                       for name in ("name", "table_group0", "physical_write_group", "wire_format"))
                or not isinstance(family, str) or not family
                or not isinstance(original, dict) or not original or not isinstance(inputs, dict)
                or not qualifiers or any(not isinstance(name, str) or not name for name in qualifiers)
                or not baseline_qualifiers
                or any(not isinstance(name, str) or not name for name in baseline_qualifiers)
                or len(set(qualifiers)) != len(qualifiers)
                or len(set(baseline_qualifiers)) != len(baseline_qualifiers)
                or not set(baseline_qualifiers).issubset(qualifiers)
                or not set(original).issubset(inputs)
                or any(inputs[name] != value for name, value in original.items())):
            raise Refused("generated write v4 target contract is malformed")
        identity = tuple(sorted(target_identity.items()))
        if identity in identities:
            raise Refused("generated write v4 source cohort has duplicate targets")
        identities.add(identity)
        case_proof = {}
        for operation, value in inputs.items():
            if not isinstance(operation, str) or not operation or value is not None and not isinstance(value, bytes):
                raise Refused("generated write v4 inputs are malformed")
            scalar = _v4_callable("public_scalar")(target, operation, value)
            if not isinstance(scalar, str) or not scalar:
                raise Refused("generated write v4 public scalar is malformed")
            case_proof[operation] = {"value_hex": None if value is None else value.hex(), "public_scalar": scalar}
        source_cohort.append({**target_identity, "case_family": family, "cases": list(inputs),
                              "case_inputs": case_proof, "qualifiers": list(qualifiers)})
        # Preserve every source-format baseline case and its original aliases,
        # rather than freezing the number of tags a historical release emits.
        original_subset += 3 * len(baseline_qualifiers) * len(original)
        for carrier in ("tiff_little", "tiff_big", "jpeg"):
            for qualifier in qualifiers:
                try:
                    target_directory = tuple(_v4_callable("directory_path")(target, qualifier, directories))
                except (TypeError, ValueError) as exc:
                    raise Refused("generated write v4 directory path is malformed") from exc
                if any(not isinstance(part, str) or not part for part in target_directory):
                    raise Refused("generated write v4 directory path is malformed")
                for operation, case in case_proof.items():
                    extended = operation not in original
                    coverage = ("extended_" if extended else "") + (
                        "selected_directory_" if target_directory else "baseline_") + family
                    row = (carrier, identity, qualifier, tuple(target_directory), family,
                           coverage, case["public_scalar"], operation)
                    if row in expected:
                        raise Refused("generated write v4 source cohort has duplicate rows")
                    expected.add(row)
    if not expected or not 0 < original_subset <= len(expected):
        raise Refused("generated write v4 source cohort has no complete baseline")
    return source_cohort, directories, expected, original_subset


def _matrix_counts(expected: set[tuple[Any, ...]]) -> tuple[dict[str, int], dict[str, int], dict[str, int]]:
    """Counts are projections of the complete authenticated row identities."""
    return (dict(Counter(row[4] for row in expected)),
            dict(Counter(row[5] for row in expected)),
            dict(Counter(row[2].split(":", 1)[0] for row in expected)))


def _matrix_report(path: Path, *, args: argparse.Namespace, native_perl: Path, native_lib: Path,
                   writer: dict[str, Any], paths: dict[str, Path], pin: Path) -> dict[str, Any]:
    value = _json(path)
    contract, matrix_native, cohort, rows = value.get("contract"), value.get("native_identity"), value.get("cohort"), value.get("rows")
    source_cohort, directories, expected, original_subset = _matrix_contract(paths)
    expected_families, expected_coverage, expected_qualifiers = _matrix_counts(expected)
    if cohort != source_cohort or value.get("explicit_directories") != list(directories):
        raise Refused("generated write matrix cohort differs from current v4 source operands")
    actual: set[tuple[Any, ...]] = set()
    if not isinstance(rows, list):
        raise Refused("generated write matrix rows are malformed")
    for row in rows:
        driver = row.get("driver_result") if isinstance(row, dict) else None
        target = row.get("target") if isinstance(row, dict) else None
        if (not isinstance(row, dict) or row.get("state") not in {"passed", "failed"}
                or not isinstance(target, dict) or set(target) != {"raw_tag_id", "name", "table_group0", "physical_write_group", "wire_format"}
                or type(target.get("raw_tag_id")) is not int
                or any(not isinstance(target.get(name), str) or not target[name]
                       for name in ("name", "table_group0", "physical_write_group", "wire_format"))
                or not isinstance(row.get("carrier"), str) or not isinstance(row.get("requested_name"), str)
                or not isinstance(row.get("target_directory"), list) or any(not isinstance(item, str) or not item for item in row["target_directory"])
                or not isinstance(row.get("case_family"), str) or not isinstance(row.get("coverage_family"), str)
                or not isinstance(row.get("public_scalar"), str) or not isinstance(row.get("operation"), str)
                or not isinstance(row.get("output"), str) or not isinstance(driver, dict) or driver.get("ok") is not True
                or driver.get("output") != row["output"] or driver.get("warnings") != [] or "error" in driver):
            raise Refused("generated write matrix row lacks an actual successful public-driver result")
        actual.add((row["carrier"], tuple(sorted(target.items())), row["requested_name"], tuple(row["target_directory"]),
                    row["case_family"], row["coverage_family"], row["public_scalar"], row["operation"]))
    family_counts = {family: sum(row.get("case_family") == family for row in rows) for family in sorted({row.get("case_family") for row in rows if isinstance(row, dict)})}
    coverage_counts = {family: sum(row.get("coverage_family") == family for row in rows) for family in sorted({row.get("coverage_family") for row in rows if isinstance(row, dict)})}
    qualifier_counts = {name: sum(row.get("requested_name", "").split(":", 1)[0] == name for row in rows)
                        for name in sorted({row.get("requested_name", "").split(":", 1)[0] for row in rows if isinstance(row, dict)})}
    native_sources = _native_writer_sources(native_lib)
    if (value.get("instrument") != "generated_scalar_write_matrix_v4" or value.get("route") != "public-api"
            or not isinstance(matrix_native, dict) or not isinstance(matrix_native.get("result"), dict)
            or not isinstance(matrix_native.get("command"), list) or matrix_native["result"].get("exiftool_version") != args.release
            or matrix_native.get("command", [None, None])[:2] != [str(native_perl), "-I" + str(native_lib)]
            or matrix_native["result"].get("source_sha256") != native_sources or value.get("source_commit") != args.source_commit
            or not isinstance(contract, dict) or contract.get("mode") != "selected-release-rehearsal"
            or contract.get("release") != args.release or contract.get("ledger_exiftool_version") != args.release
            or contract.get("pin") != str(pin.resolve()) or contract.get("pin_sha256") != _sha(pin)
            or value.get("test_binary_path") != writer["path"] or value.get("test_binary_sha256") != writer["sha256"]
            or value.get("ledger_sha256") != _sha(paths["final_ledger"]) or value.get("rules_sha256") != _sha(paths["final_rules"])
            or value.get("declared_by_case_family") != family_counts or value.get("declared_by_coverage_family") != coverage_counts
            or value.get("declared_by_qualifier") != qualifier_counts or family_counts != expected_families
            or coverage_counts != expected_coverage or qualifier_counts != expected_qualifiers or type(value.get("declared")) is not int
            or value["declared"] != len(expected) or type(value.get("passed")) is not int
            or not 0 <= value["passed"] <= value["declared"] or len(rows) != value["declared"]
            or actual != expected or len(actual) != len(rows) or value["passed"] != sum(row["state"] == "passed" for row in rows)):
        raise Refused("generated write matrix report is not bound to the selected v4 build and native release")
    return {"path": str(path), "sha256": _sha(path), "declared": value["declared"], "passed": value["passed"],
            "mismatched": value["declared"] - value["passed"], "original_subset": original_subset,
            "source_contract": _source_proof(paths)}


def write(args: argparse.Namespace, *, run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run) -> dict[str, Any]:
    checkout, target, report, perl, _native_source, native_lib, identity = _common(args, run)
    if (checkout / ".exiftool-version").read_text(encoding="utf-8") != args.release + "\n":
        raise Refused("write checkout is not pinned to selected release")
    previous = _prior(report, "build", args, identity, checkout)
    generated = _validate_artifacts(checkout, previous.get("generated_artifacts"))
    writer = _build_binary(previous, target, "writer_binary", "writer driver")
    fixtures, fixture_digest, _corpus = _write_fixtures(Path(args.fixture_manifest), target)
    matrix_sources = _matrix_source_artifacts(checkout)
    _, _, expected, original_subset = _matrix_contract(matrix_sources)
    ledger, rules = matrix_sources["final_ledger"], matrix_sources["final_rules"]
    source_proof = _source_proof(matrix_sources)
    native_sources = _native_writer_sources(native_lib)
    pin = checkout / ".exiftool-version"
    matrix_root = report.parent / "raw" / "write-matrix"
    if matrix_root.exists() or matrix_root.is_symlink():
        raise Refused("write matrix evidence directory already exists")
    records, matrix_reports = [], []
    env = _environment(perl, native_lib, target)
    env["OXIDEX_ALLOW_DIRTY_TREE"] = "1"
    for index, fixture in enumerate(fixtures):
        output = matrix_root / f"{index:04d}" / "report.json"
        command = [sys.executable, str(checkout / "tools/exiftool-tables/generated_tiff_write_matrix.py"),
                   "--test-binary", writer["path"], "--perl", str(perl), "--lib", str(native_lib),
                   "--jpeg-base", fixture["corpus_path"], "--output", str(output), "--ledger", str(ledger),
                   "--rules", str(rules), "--route", "public-api", "--rehearsal-release", args.release,
                   "--rehearsal-pin", str(checkout / ".exiftool-version")]
        record = _run(command, cwd=checkout, env=env, run=run)
        records.append(record)
        if record["state"] != "ok":
            _raw(report, "write", {"commands": records, "state": "failed"})
            raise Refused("generated write matrix command failed")
        if not output.is_file() or output.is_symlink():
            _raw(report, "write", {"commands": records, "state": "failed"})
            raise Refused("generated write matrix did not publish a report")
        matrix_reports.append(_matrix_report(output, args=args, native_perl=perl, native_lib=native_lib, writer=writer,
                                             paths=matrix_sources, pin=pin))
    raw = _raw(report, "write", {"commands": records, "matrix_reports": matrix_reports,
                                  "state": "ok" if all(record["state"] == "ok" for record in records) else "failed"})
    _verify_staged_fixtures(fixtures)
    _prior(report, "build", args, identity, checkout)
    _validate_artifacts(checkout, generated)
    _build_binary(previous, target, "writer_binary", "writer driver")
    if _source_proof(matrix_sources) != source_proof:
        raise Refused("generated writer source contract changed during matrix comparison")
    if _native_writer_sources(native_lib) != native_sources:
        raise Refused("selected native writer source changed during matrix comparison")
    for matrix in matrix_reports:
        if _sha(Path(matrix["path"])) != matrix["sha256"]:
            raise Refused("generated write matrix evidence changed during comparison")
    declared = sum(row["declared"] for row in matrix_reports)
    passed = sum(row["passed"] for row in matrix_reports)
    mismatched = declared - passed
    result = {**_base("write", args, checkout, identity), "state": "passed" if mismatched == 0 else "failed",
              "denominator": declared, "native_release": args.release,
              "native_probe_sha256": args.native_probe_sha256,
              "comparison": {"kind": "oxidex_vs_native", "native_release": args.release,
                             "matched": passed, "mismatched": mismatched},
              "generated_artifacts": generated, "writer_binary": writer,
              "fixtures": {"manifest": str(Path(args.fixture_manifest).absolute()),
                           "manifest_sha256": fixture_digest, "entries": fixtures},
              "matrix_reports": matrix_reports, "raw_report": raw,
              "write_mode": {"kind": "selected-release-live-native", "release": args.release,
                             "ledger_sha256": source_proof["final_ledger"]["sha256"],
                             "rules_sha256": source_proof["final_rules"]["sha256"],
                             "source_contract": source_proof,
                             "original_subset": original_subset, "expanded_matrix": len(expected)},
              "scope": "selected-release public-api generated scalar cohort on staged JPEG fixtures plus synthetic little- and big-endian TIFF carriers",
              "limitations": ["Only the emitted TIFF/JPEG scalar cohort is exercised.",
                              "Fresh/empty EXIF, other writer grammars, and non-JPEG formats remain outside this rehearsal stage."]}
    _atomic(report, result)
    return result


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__); sub = parser.add_subparsers(dest="stage", required=True)
    for name in ("generate", "build", "read", "write"):
        command = sub.add_parser(name)
        for option in ("checkout", "target", "report", "release", "source-commit", "native-source", "native-lib", "native-perl"):
            command.add_argument("--" + option, required=True)
        if name in {"read", "write"}: command.add_argument("--fixture-manifest", required=True); command.add_argument("--native-probe-sha256", required=True)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = write(args) if args.stage == "write" else globals()[args.stage](args)
        print(json.dumps(result, sort_keys=True)); return 0 if result["state"] == "passed" else 2
    except (Refused, OSError, ValueError) as exc:
        print(f"refused: {exc}", file=sys.stderr); return 2


if __name__ == "__main__":
    raise SystemExit(main())
