#!/usr/bin/env python3
"""Concrete, non-promoting commands for a version-rehearsal stage.

This is deliberately an adapter, not the scheduler.  The scheduler owns the
selected releases and invokes this program once per stage.  Every mutation is
limited to that release's owned checkout and target directory.  In particular,
the adapter never falls back to the repository's normal ExifTool pin.
"""
from __future__ import annotations

import argparse
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
import version_rehearsal as rehearsal
import version_rehearsal_executor as executor

RELEASE = re.compile(r"^[0-9]+\.[0-9]+$")
OID = rehearsal.GIT_OID_RE
COMMAND_TIMEOUT_SECONDS = 3600


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
    for item in artifacts.ARTIFACTS:
        path = _regular(checkout / item.path, f"generated artifact {item.path}")
        rows.append({"path": item.path, "sha256": _sha(path), "bytes": path.stat().st_size})
    return rows


def _validate_artifacts(checkout: Path, rows: Any) -> list[dict[str, Any]]:
    if not isinstance(rows, list) or len(rows) != len(artifacts.ARTIFACTS):
        raise Refused("generated artifact proof is incomplete")
    expected = [item.path for item in artifacts.ARTIFACTS]
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


def _prior(report: Path, stage: str, args: argparse.Namespace, identity: dict[str, Any]) -> dict[str, Any]:
    value = _json(report.parent / f"{stage}.json")
    if (value.get("schema") != executor.SCHEMA or value.get("kind") != executor.RESULT_KIND
            or value.get("stage") != stage or value.get("state") != "passed" or value.get("release") != args.release
            or value.get("source_commit") != args.source_commit or value.get("native_identity") != identity):
        raise Refused("prior stage is not bound to this release, source, and native identity")
    return value


def build(args: argparse.Namespace, *, run: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run) -> dict[str, Any]:
    checkout, target, report, perl, _source, native_lib, identity = _common(args, run)
    if (checkout / ".exiftool-version").read_text(encoding="utf-8") != args.release + "\n":
        raise Refused("build checkout is not pinned to selected release")
    generated = _validate_artifacts(checkout, _prior(report, "generate", args, identity).get("generated_artifacts"))
    record = _run(["cargo", "build", "--message-format=json", "--bin", "oxidex"], cwd=checkout,
                  env=_environment(perl, native_lib, target), run=run)
    raw = _raw(report, "build", record)
    if record["state"] != "ok":
        raise Refused("cargo build failed")
    executable: Path | None = None
    for line in record["stdout"].splitlines():
        try: row = json.loads(line)
        except json.JSONDecodeError: continue
        if row.get("reason") == "compiler-artifact" and row.get("target", {}).get("name") == "oxidex" and isinstance(row.get("executable"), str):
            candidate = _regular(Path(row["executable"]), "cargo JSON oxidex executable")
            if executable is not None and executable != candidate: raise Refused("cargo JSON emitted multiple oxidex executables")
            executable = candidate
    if executable is None or not executable.is_relative_to(target):
        raise Refused("cargo JSON did not identify an isolated oxidex executable")
    result = {**_base("build", args, checkout, identity), "state": "passed", "denominator": 1,
              "generated_artifacts": generated, "binary": {"path": str(executable), "sha256": _sha(executable), "bytes": executable.stat().st_size}, "raw_report": raw}
    _atomic(report, result); return result


def _fixtures(manifest: Path, target: Path) -> tuple[list[dict[str, Any]], str, Path]:
    source = _json(manifest)
    if source.get("schema") != 1 or source.get("kind") != "oxidex_version_rehearsal_fixture_manifest" or not isinstance(source.get("fixtures"), list) or not source["fixtures"]:
        raise Refused("fixture manifest schema is unsupported")
    digest = _sha(manifest); corpus = target / "rehearsal-fixtures" / digest
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


def _verify_staged_fixtures(rows: list[dict[str, Any]]) -> None:
    for row in rows:
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
    previous = _prior(report, "build", args, identity)
    generated = _validate_artifacts(checkout, previous.get("generated_artifacts"))
    binary = previous.get("binary")
    if not isinstance(binary, dict) or not isinstance(binary.get("path"), str) or not isinstance(binary.get("sha256"), str): raise Refused("build binary proof is malformed")
    executable = _regular(Path(binary["path"]), "built oxidex executable")
    if _sha(executable) != binary["sha256"] or executable.stat().st_size != binary.get("bytes"): raise Refused("built oxidex executable changed")
    fixtures, fixture_digest, corpus = _fixtures(Path(args.fixture_manifest), target)
    comparison = report.parent / "raw" / "read-conformance.json"
    command = [sys.executable, str(checkout / "tools" / "exiftool-tables" / "conformance.py"), str(corpus), "--recursive", "--exiftool-dir", str(native_source), "--oxidex", str(executable), "--min-files", str(len(fixtures)), "--min-tags", "1", "--json-out", str(comparison)]
    env = _environment(perl, native_lib, target); env["OXIDEX_ALLOW_DIRTY_TREE"] = "1"
    record = _run(command, cwd=checkout, env=env, run=run); raw = _raw(report, "read", record)
    if record["state"] != "ok" or not comparison.is_file(): raise Refused("actual conformance.py comparison failed")
    _verify_staged_fixtures(fixtures)
    data = _json(comparison); _verify_conformance_scope(data, fixtures); per_format = data["per_format"]
    matched = mismatched = 0
    for counts in per_format.values():
        if not isinstance(counts, dict): raise Refused("conformance format counts are malformed")
        for key in ("matched", "value_diff", "missing", "renames", "extra"):
            if type(counts.get(key)) is not int or counts[key] < 0: raise Refused("conformance count is malformed")
        matched += counts["matched"]; mismatched += counts["value_diff"] + counts["missing"] + counts["renames"] + counts["extra"]
    denominator = matched + mismatched
    state = "passed" if denominator > 0 and mismatched == 0 else "failed"
    result = {**_base("read", args, checkout, identity), "state": state, "denominator": denominator,
              "native_release": args.release, "native_probe_sha256": args.native_probe_sha256,
              "comparison": {"kind": "oxidex_vs_native", "native_release": args.release, "matched": matched, "mismatched": mismatched},
              "generated_artifacts": generated, "binary": {"path": str(executable), "sha256": _sha(executable), "bytes": executable.stat().st_size},
              "fixtures": {"manifest": str(Path(args.fixture_manifest).absolute()), "manifest_sha256": fixture_digest, "entries": fixtures}, "raw_report": raw,
              "conformance_report": {"path": str(comparison), "sha256": _sha(comparison)}}
    _atomic(report, result)
    return result


def write(args: argparse.Namespace) -> dict[str, Any]:
    report = Path(args.report).absolute()
    if report.exists() or report.is_symlink(): raise Refused("stage report already exists")
    result = {"schema": executor.SCHEMA, "kind": executor.RESULT_KIND, "stage": "write", "release": args.release,
              "state": "unsupported", "reason": "generated writer public acceptance contract is not implemented; no historical baseline substituted"}
    _atomic(report, result); return result


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__); sub = parser.add_subparsers(dest="stage", required=True)
    for name in ("generate", "build", "read"):
        command = sub.add_parser(name)
        for option in ("checkout", "target", "report", "release", "source-commit", "native-source", "native-lib", "native-perl"):
            command.add_argument("--" + option, required=True)
        if name == "read": command.add_argument("--fixture-manifest", required=True); command.add_argument("--native-probe-sha256", required=True)
    command = sub.add_parser("write"); command.add_argument("--report", required=True); command.add_argument("--release", required=True)
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
