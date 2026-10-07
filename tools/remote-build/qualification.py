#!/usr/bin/env python3
"""Run exact-HEAD Task19 and the corpus read gate on one Linux amd64 Spot VM.

The caller must supply an already-provisioned Task19 output root whose plan
is bound to this clean commit. This script transfers those immutable inputs,
performs all expensive work remotely, and downloads hash-checked receipts.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import math
import os
import re
from pathlib import Path
import secrets
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from ops_paths import ops_root  # noqa: E402
from lib.worker_selection import select_worker  # noqa: E402
from lib.ssh_transport import DirectTransport, identity as ssh_identity  # noqa: E402
from lib.remote_build import verify_builder_admission, unique_run_id, source_sync_command  # noqa: E402
from qualification_source import verify_source, REMOTE_TRUSTED_SIGNERS  # noqa: E402

ROWS = ("same-pin-{pin}", "11.78-to-12.64", "12.64-to-11.78")


def sha(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def git(*args: str) -> str:
    return subprocess.check_output(["git", "-C", str(ROOT), *args], text=True).strip()


def qualification_module():
    sys.path.insert(0, str(ROOT / "tools/exiftool-tables"))
    import version_transition_qualification as module
    return module


def exact_inputs(output: Path) -> tuple[str, str, list[Path]]:
    """On Spot, reject stale or incomplete Task19 inputs before any row runs."""
    q = qualification_module()
    if output.is_symlink() or not output.is_dir() or not output.is_relative_to(ops_root()):
        raise ValueError("output must be a real directory beneath OXIDEX_OPS_DIR")
    caller = q.snapshot_caller(ROOT)
    head = git("rev-parse", "HEAD")
    if caller["head"] != head:
        raise ValueError("caller HEAD changed during qualification input preflight")
    verify_source(ROOT, head, source_signers(), source_attestation())
    pin = (ROOT / ".exiftool-version").read_text().strip()
    matrix = q.materialize_matrix(q.load_matrix(q.CANONICAL_MATRIX, pin),
                                  output_root=output, target_root=ops_root() / "targets", run_id="preflight")
    policy = output / "read-policy-input.json"
    q._read_policy_input(policy, {r["id"] for r in matrix["rows"]})
    for row in matrix["rows"]:
        for side in q.SIDES:
            frozen = q._freeze_side_inputs(row, side)
            if frozen["identity"]["documents"]["plan"].get("repository_commit") != head:
                raise ValueError(f"{row['id']} {side} input plan is not bound to HEAD {head}")
    return head, pin, [policy, output / "provisioned"]


def transport_inputs(output: Path) -> tuple[str, str, list[Path], Path]:
    """Discover upload paths locally; Task19 validates their bytes on Spot."""
    root = ops_root()
    if output.resolve() != output or not output.is_dir() or not output.is_relative_to(root):
        raise ValueError("qualification output must be a real ops-root directory")
    policy, provisioned = output / "read-policy-input.json", output / "provisioned"
    if not policy.is_file() or not provisioned.is_dir():
        raise ValueError("prepared Task19 policy or provisioned directory is missing")
    head = git("rev-parse", "HEAD")
    if git("status", "--porcelain"):
        raise ValueError("qualification source must be clean before upload")
    signer = source_signers()
    verify_source(ROOT, head, signer, source_attestation())
    if not signer.is_file():
        raise ValueError("maintainer allowed signers file is absent")
    # JSON references only tell the transport which bytes to send. No local
    # Task19 plan/policy validation, source hashing or compression occurs here.
    paths = input_paths([policy, provisioned], root)
    if not paths:
        raise ValueError("prepared Task19 upload has no files")
    return head, (ROOT / ".exiftool-version").read_text().strip(), paths, signer


def source_attestation() -> Path | None:
    value = os.environ.get("OXIDEX_QUALIFICATION_SOURCE_ATTESTATION")
    if value:
        return Path(value)
    staged = Path("/src/source-attestation")
    return staged if staged.exists() else None


def source_signers() -> Path:
    if ROOT == Path("/target/checkout"):
        return REMOTE_TRUSTED_SIGNERS
    uploaded = Path("/src/maintainer.allowed_signers")
    if uploaded.is_file():
        return uploaded
    return Path(git("config", "--path", "--get", "gpg.ssh.allowedSignersFile"))


def transfer_identity(path: Path) -> tuple[int, int, int, int]:
    """Catch ordinary concurrent writes without reading payload bytes locally."""
    status = path.stat()
    return status.st_dev, status.st_ino, status.st_size, status.st_mtime_ns


def input_paths(seeds: list[Path], root: Path) -> list[Path]:
    """Include the prepared bundle plus every ops-root file path it names."""
    pending = seeds[:]
    selected: set[Path] = set()
    while pending:
        raw = pending.pop()
        if raw.is_symlink():
            raise ValueError(f"qualification input symlink is forbidden: {raw}")
        path = raw.resolve()
        if not path.is_relative_to(root):
            raise ValueError(f"qualification input is outside OXIDEX_OPS_DIR: {path}")
        if path in selected:
            continue
        if not path.exists():
            raise ValueError(f"qualification input absent or symlinked: {path}")
        selected.add(path)
        if path.is_dir():
            pending.extend(path.iterdir())
        elif path.suffix == ".json" and path.stat().st_size < 32 * 1024 * 1024:
            try:
                document = json.loads(path.read_text())
            except (ValueError, UnicodeError):
                continue
            def visit(value, key=""):
                if isinstance(value, str) and value.startswith(str(root) + "/"):
                    dependency = Path(value)
                    if dependency.exists():
                        pending.append(dependency)
                elif (isinstance(value, str) and value.startswith("/")
                      and key in {"path", "fixture", "source_root", "source", "archive", "manifest"}):
                    raise ValueError(f"qualification input {key} is outside OXIDEX_OPS_DIR: {value}")
                elif isinstance(value, dict):
                    for name, item in value.items():
                        visit(item, name)
                elif isinstance(value, list):
                    for item in value:
                        visit(item, key)
            visit(document)
    return sorted(path for path in selected if path.is_file())


def prepare_archive(output: Path, destination: Path, bundle: Path) -> tuple[str, str]:
    root = ops_root()
    seeds = [output / "read-policy-input.json", output / "provisioned"]
    # The expensive qualification may begin hours later. Detect a writer that
    # changes a validated floor, plan, or fixture while the transfer is packed.
    before = {path: sha(path) for path in input_paths(seeds, root)}
    head, pin, seeds = exact_inputs(output)
    if {path: sha(path) for path in input_paths(seeds, root)} != before:
        raise ValueError("qualification inputs changed during preflight")
    signers = git("config", "--path", "--get", "gpg.ssh.allowedSignersFile")
    signers_path = Path(signers)
    if not signers_path.is_file():
        raise ValueError("maintainer allowed signers file is absent")
    signer_sha = sha(signers_path)
    signer_bytes = signers_path.read_bytes()
    if hashlib.sha256(signer_bytes).hexdigest() != signer_sha:
        raise ValueError("maintainer allowed signers changed during packaging")
    bundle_sha = sha(bundle)
    source_identity = verify_source(ROOT, head, source_signers(), source_attestation(),
                                    bundle_sha256=bundle_sha)
    try:
        with tarfile.open(destination, "w:gz", compresslevel=3) as archive:
            def add_frozen(path: Path, name: str, expected: str) -> None:
                # Archive the same bytes we hash, never ask tarfile to reopen
                # a mutable path after its preflight identity was checked.
                with tempfile.TemporaryFile(dir=destination.parent) as frozen, path.open("rb") as live:
                    digest = hashlib.sha256()
                    size = 0
                    for block in iter(lambda: live.read(1024 * 1024), b""):
                        digest.update(block)
                        frozen.write(block)
                        size += len(block)
                    if digest.hexdigest() != expected:
                        raise ValueError(f"qualification input changed while freezing: {path}")
                    frozen.seek(0)
                    member = tarfile.TarInfo(name)
                    member.size = size
                    member.mode = 0o644
                    archive.addfile(member, frozen)
            add_frozen(bundle, "repository.bundle", bundle_sha)
            if source_attestation() is not None:
                for item in source_attestation().iterdir():
                    if item.is_file():
                        add_frozen(item, "source-attestation/" + item.name, sha(item))
            signer_member = tarfile.TarInfo("maintainer.allowed_signers")
            signer_member.size = len(signer_bytes)
            signer_member.mode = 0o644
            archive.addfile(signer_member, io.BytesIO(signer_bytes))
            for path, digest in before.items():
                add_frozen(path, str(Path("ops") / path.relative_to(root)), digest)
        if ({path: sha(path) for path in input_paths(seeds, root)} != before
                or sha(signers_path) != signer_sha or sha(bundle) != bundle_sha):
            raise ValueError("qualification inputs changed while creating the transfer archive")
        if verify_source(ROOT, head, source_signers(), source_attestation(),
                         bundle_sha256=bundle_sha) != source_identity:
            raise ValueError("qualification source attestation changed during packaging")
    except Exception:
        destination.unlink(missing_ok=True)
        raise
    (output / "source-identity.json").write_text(json.dumps(source_identity, sort_keys=True) + "\n")
    return head, pin


def safe_extract(archive: Path, destination: Path) -> None:
    """Extract only into a new staging directory, never into live evidence."""
    if destination.exists():
        raise ValueError(f"remote receipt staging path already exists: {destination}")
    with tarfile.open(archive, "r:gz") as source:
        members = source.getmembers()
        if any((not member.isfile() and not member.isdir()) or not member.name
               or member.name.startswith("/") or ".." in Path(member.name).parts
               for member in members):
            raise ValueError("remote receipt archive contains an unsafe member")
        destination.mkdir(parents=True)
        try:
            # Regular files and directories only; this is safe on all stated
            # Python 3.11 patch versions without tarfile's newer filter API.
            for member in members:
                target = destination / member.name
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                target.parent.mkdir(parents=True, exist_ok=True)
                stream = source.extractfile(member)
                if stream is None:
                    raise ValueError(f"remote receipt has no bytes: {member.name}")
                with stream, target.open("xb") as output:
                    copied = 0
                    for block in iter(lambda: stream.read(1024 * 1024), b""):
                        output.write(block)
                        copied += len(block)
                    if copied != member.size:
                        raise ValueError(f"remote receipt member was truncated: {member.name}")
        except Exception:
            shutil.rmtree(destination)
            raise


def corpus_commands(corpus: Path, instrument: Path, perl: Path, exiftool: Path,
                    samples: Path, floor: int) -> list[list[str]]:
    """Bind the CLI's output *directories* to its nested proof and receipt files."""
    build = corpus / "build"
    observe = corpus / "observations"
    receipt = observe / "receipt.json"
    return [
        [sys.executable, str(instrument), "build", "--output", str(build)],
        [sys.executable, str(instrument), "observe", "--build-proof", str(build / "build-proof.json"),
         "--perl", str(perl), "--exiftool-dir", str(exiftool), "--corpus", str(samples),
         "--output", str(observe), "--min-files", str(floor)],
        [sys.executable, str(instrument), "verify", "--receipt", str(receipt)],
        [sys.executable, str(ROOT / "tools/ci/read_regression_gate.py"), "--receipt", str(receipt)],
    ]


def task19_row_exit(code: int, row: str, receipt: Path, result: dict) -> int | None:
    """Retain unknown outcomes, including killed processes with unproven cleanup."""
    if code not in (0, 2, 3):
        result["status"] = "RUNNING_RETAINED"
        result["unconfirmed_row"] = {"id": row, "exit_code": code,
                                     "receipt_root": str(receipt),
                                     "log": str(receipt.parent / f"{receipt.name}.log")}
        # The controller recognizes 4/5 as unconfirmed. Normalize a signal,
        # keyboard interrupt, or unexpected crash to 5, but preserve its raw
        # return code above for the operator inspecting the retained stage.
        return code if code in (4, 5) else 5
    if code == 2:
        raise ValueError(f"Task19 {row} exit {code}; see {receipt.name}.log")
    return None


def verify_source_result_binding(output: Path, head: str, signer: Path, bundle: Path,
                                 attestation: Path | None, run_id: str | None,
                                 expected: dict, packed: dict | None = None) -> None:
    """Match controller, packed and archived source identities before PASS."""
    bundle_sha = sha(bundle) if attestation is not None else None
    current = verify_source(ROOT, head, signer, attestation,
                            bundle_sha256=bundle_sha, run_id=run_id)
    archived_attestation = output / "source-attestation" if attestation is not None else None
    archived = verify_source(ROOT, head, signer, archived_attestation,
                             bundle_sha256=bundle_sha, run_id=run_id)
    if (current != expected or archived != expected
            or (packed is not None and packed != expected)
            or json.loads((output / "source-identity.json").read_text()) != expected
            or json.loads((output / "remote-qualification.json").read_text()).get("source_identity") != expected):
        raise RuntimeError("packed or archived qualification source identity differs")


def verify_archived_source_members(archive: Path, output: Path,
                                   attested: bool) -> None:
    """Check source evidence bytes in a result tar against its live origin."""
    prefix = str(output.relative_to(ops_root())) + "/"
    names = {"source-identity.json"}
    if attested:
        names.update("source-attestation/" + name for name in
                     ("document.json", "document.sig", "github-commit.json",
                      "pgp-proof.txt", "integration-ref.json"))
    observed = set()
    with tarfile.open(archive, "r:gz") as stream:
        for member in stream:
            if not member.name.startswith(prefix):
                continue
            name = member.name[len(prefix):]
            if name not in names:
                continue
            if name in observed or not member.isfile():
                raise RuntimeError("result archive duplicates source evidence")
            extracted = stream.extractfile(member)
            if extracted is None or extracted.read() != (output / name).read_bytes():
                raise RuntimeError("result archive source evidence differs")
            observed.add(name)
    if observed != names:
        raise RuntimeError("result archive lacks bound source evidence")


def replay_archived_corpus(archive: Path, output: Path, expected_head: str) -> dict:
    """Replay the exact packed receipt and published-read gate on Spot."""
    if git("rev-parse", "HEAD") != expected_head or git("status", "--porcelain"):
        raise ValueError("archive replay source is not the exact clean candidate")
    source_identity = verify_source(ROOT, expected_head, source_signers(), source_attestation(),
                  bundle_sha256=sha(Path("/src/repository.bundle")) if source_attestation() else None)
    verify_source_result_binding(output, expected_head, source_signers(),
        Path("/src/repository.bundle"), source_attestation(), None, source_identity)
    verify_archived_source_members(archive, output, source_attestation() is not None)
    member_name = str(output.relative_to(ops_root()) / "corpus-read/observations/receipt.json")
    with tarfile.open(archive, "r:gz") as stream:
        matches = [member for member in stream.getmembers() if member.name == member_name]
        if len(matches) != 1 or not matches[0].isfile():
            raise ValueError("results archive lacks one regular corpus receipt")
        member = stream.extractfile(matches[0])
        if member is None:
            raise ValueError("results archive corpus receipt has no bytes")
        with member:
            packed_bytes = member.read()
    packed_sha = hashlib.sha256(packed_bytes).hexdigest()
    receipt = output / "corpus-read/observations/receipt.json"
    if sha(receipt) != packed_sha:
        raise ValueError("archived corpus receipt differs from the gated remote file")
    document = json.loads(packed_bytes)
    pin = (ROOT / ".exiftool-version").read_text().strip()
    if (not isinstance(document, dict) or not isinstance(document.get("producer"), dict)
            or document["producer"].get("source_commit") != expected_head):
        raise ValueError("archived corpus receipt measures another source")
    sys.path.insert(0, str(ROOT / "tools/ci"))
    import read_regression_gate as gate
    measurements = ROOT / "docs/public/measurements"
    # An anonymous file prevents a concurrent writer from replacing the live
    # receipt between the archive digest check and the gate's own read.
    descriptors = Path("/proc/self/fd") if Path("/proc/self/fd").is_dir() else Path("/dev/fd")
    if not descriptors.is_dir():
        raise ValueError("anonymous receipt descriptor path is unavailable")
    with tempfile.TemporaryFile(mode="w+b", dir=output / "corpus-read") as frozen:
        frozen.write(packed_bytes)
        frozen.flush()
        frozen.seek(0)
        published, verdict, _receipt = gate.measure(
            descriptors / str(frozen.fileno()),
            measurements / f"catalog-corpus-observed-{pin}.json",
            measurements / f"catalog-source-{pin}.json", ROOT)
    if verdict.status != "PASS":
        raise ValueError(f"archived corpus read gate returned {verdict.status}")
    q = qualification_module()
    marker_hashes = {}
    with tarfile.open(archive, "r:gz") as stream:
        for index, template in enumerate(ROWS):
            row = template.format(pin=pin)
            run_id = f"spot-{expected_head[:12]}-{index}"
            marker = output / run_id / "qualification-result.json"
            archive_name = str(marker.relative_to(ops_root()))
            found = [member for member in stream.getmembers() if member.name == archive_name]
            if len(found) != 1 or not found[0].isfile():
                raise ValueError(f"archive lacks one committed Task19 marker for {row}")
            body = stream.extractfile(found[0])
            if body is None:
                raise ValueError(f"archive cannot read Task19 marker for {row}")
            with body:
                frozen_sha = hashlib.sha256(body.read()).hexdigest()
            if sha(marker) != frozen_sha:
                raise ValueError(f"archived Task19 marker differs from retained file for {row}")
            committed = q.load_committed_result(marker)
            if committed["caller"]["head"] != expected_head or [item["id"] for item in committed["rows"]] != [row]:
                raise ValueError(f"archived Task19 marker does not replay exact head and row {row}")
            marker_hashes[row] = frozen_sha
    return {"schema": 1, "kind": "oxidex_remote_corpus_archive_replay", "status": "PASS",
            "head": expected_head, "pin": pin, "archive_sha256": sha(archive),
            "corpus_receipt_sha256": packed_sha, "corpus_files": published.corpus_files,
            "task19_markers": marker_hashes}


def pack_results(output: Path, expected_head: str) -> dict:
    """Freeze completed row/corpus receipts under the immutable target mount."""
    if git("rev-parse", "HEAD") != expected_head or git("status", "--porcelain"):
        raise ValueError("result packer is not running from the exact clean candidate")
    source_identity = verify_source(ROOT, expected_head, source_signers(), source_attestation(),
                                    bundle_sha256=sha(Path("/src/repository.bundle")) if source_attestation() else None)
    verify_source_result_binding(output, expected_head, source_signers(),
        Path("/src/repository.bundle"), source_attestation(), None, source_identity)
    if not output.is_relative_to(ops_root()) or not output.is_dir():
        raise ValueError("remote qualification output escaped its ops root")
    archive = Path("/target/results.tar.gz")
    if archive.exists() or archive.is_symlink():
        raise ValueError("remote result archive already exists")
    names = ["read-policy-input.json", "preparation.json", "remote-qualification.json",
             "source-identity.json", "source-attestation",
             "corpus-read", *[f"spot-{expected_head[:12]}-{i}" for i in range(3)],
             *[f"spot-{expected_head[:12]}-{i}.log" for i in range(3)]]
    present = []
    for name in names:
        path = output / name
        if path.exists() or path.is_symlink():
            present.append(path)
    if not present:
        raise ValueError("remote qualification has no receipts to preserve")
    with tarfile.open(archive, "w:gz", compresslevel=3) as stream:
        for root in present:
            for path in [root, *(root.rglob("*") if root.is_dir() else [])]:
                if path.is_symlink() or not (path.is_file() or path.is_dir()):
                    raise ValueError(f"result archive member is unsafe: {path}")
                stream.add(path, arcname=str(path.relative_to(ops_root())), recursive=False)
    return {"schema": 1, "kind": "oxidex_remote_qualification_archive",
            "source_identity": source_identity,
            "archive_sha256": sha(archive), "archive_bytes": archive.stat().st_size}


def configure_local_signing(target_root: Path, signer_file: Path) -> None:
    """Configure signed generated checkouts in the actual candidate repository."""
    # The signing key is generated on this isolated VM and used only for the
    # owned measurement checkouts. It is never transferred back or published.
    signing = target_root / "remote-signing"
    signing.mkdir(parents=True, exist_ok=False)
    key = signing / "id_ed25519"
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)], check=True)
    allowed = signing / "allowed_signers"
    maintainer_signers = signer_file.read_text()
    if not maintainer_signers.strip():
        raise ValueError("maintainer allowed signers file is empty")
    allowed.write_text(maintainer_signers.rstrip("\n") + "\n" +
                       "spot-qualification@local " + (signing / "id_ed25519.pub").read_text())
    for name, value in {"user.name": "OxiDex Spot qualification", "user.email": "spot-qualification@local",
                        "user.signingkey": str(key), "gpg.format": "ssh",
                        "gpg.ssh.allowedSignersFile": str(allowed)}.items():
        subprocess.run(["git", "-C", str(ROOT), "config", "--local", name, value], check=True)


def remote_run(output: Path, expected_head: str) -> int:
    """Container phase; source and ops root are already mounted at exact paths."""
    if git("rev-parse", "HEAD") != expected_head or git("status", "--porcelain"):
        raise ValueError("remote Git checkout is not the exact clean candidate")
    target_root = Path(os.environ.get("OXIDEX_TARGET_ROOT", str(ops_root() / "targets")))
    source_identity = verify_source(ROOT, expected_head, source_signers(), source_attestation(),
                                    bundle_sha256=sha(Path("/src/repository.bundle")) if source_attestation() else None)
    configure_local_signing(target_root, Path("/src/maintainer.allowed_signers"))
    if source_attestation() is not None:
        shutil.copytree(source_attestation(), output / "source-attestation")
    q = qualification_module()
    pin = (ROOT / ".exiftool-version").read_text().strip()
    rows = tuple(row.format(pin=pin) for row in ROWS)
    policy = output / "read-policy-input.json"
    result = {"schema": 1, "kind": "oxidex_remote_spot_qualification", "head": expected_head,
              "source_identity": source_identity,
              "pin": pin, "rows": {}, "corpus_gate": "unrun", "status": "pending"}
    summary = output / "remote-qualification.json"
    def save():
        summary.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    save()
    try:
        from importlib.util import module_from_spec, spec_from_file_location
        spec = spec_from_file_location("oxidex_remote_bootstrap", ROOT / "tools/release/bootstrap_oracle.py")
        bootstrap = module_from_spec(spec)
        spec.loader.exec_module(bootstrap)
        sys.path.insert(0, str(ROOT))
        from tools.release import approved_linux_perl
        approval = approved_linux_perl.load(
            ROOT / "tools/release/oracle-lock.json",
            envelope=Path("/src/reference/approved-linux-perl.json"),
        )
        bootstrap.provision(ops_root(), approved_perl=approval)
        from qualification_prepare import verify_prepared
        verified_preparation = verify_prepared(Path("/src/reference"), output, expected_head)
        result["preparation_sha256"] = sha(output / "preparation.json")
        result["reference_policy_sha256"] = verified_preparation["policy_reference_sha256"]
        save()
        # The expensive Task19 input validation and content-freezing archive
        # happen only inside this Spot container, before the first row.
        input_archive = output / "spot-input.tar.gz"
        frozen_head, frozen_pin = prepare_archive(output, input_archive, Path("/src/repository.bundle"))
        if frozen_head != expected_head or frozen_pin != pin:
            raise ValueError("Spot-frozen Task19 inputs differ from the candidate")
        result["input_archive_sha256"] = sha(input_archive)
        result["input_file_count"] = len(input_paths([policy, output / "provisioned"], ops_root()))
        verify_prepared(Path("/src/reference"), output, expected_head)
        save()
        lease = output / "transition.host.lock"
        lease.touch(exist_ok=True)
        for index, row in enumerate(rows):
            run_id = f"spot-{expected_head[:12]}-{index}"
            receipt = output / run_id
            arguments = ["--matrix", str(q.CANONICAL_MATRIX), "--repository", str(ROOT),
                         "--output", str(output), "--target-root", str(target_root),
                         "--lease", str(lease), "--run-id", run_id, "--only", row,
                         "--read-policy-input", str(policy),
                         "--owner-receipt", str(receipt / "lease-owner.json"),
                         "--heartbeat-receipt", str(receipt / "lease-heartbeat.jsonl"),
                         "--expiry-receipt", str(receipt / "lease-expiry.json"),
                         "--release-receipt", str(receipt / "lease-release.json"),
                         "--handoff-receipt", str(receipt / "handoff.jsonl")]
            with (output / f"{run_id}.log").open("w") as log:
                process = subprocess.run([sys.executable, str(q.__file__), *arguments], stdout=log,
                                         stderr=subprocess.STDOUT)
            retained_exit = task19_row_exit(process.returncode, row, receipt, result)
            if retained_exit is not None:
                save()
                return retained_exit
            committed = q.load_committed_result(receipt / "qualification-result.json")
            if committed["caller"]["head"] != expected_head or [r["id"] for r in committed["rows"]] != [row]:
                raise ValueError(f"Task19 {row} final marker does not bind exact head and row")
            result["rows"][row] = {"run_id": run_id, "marker_sha256": sha(receipt / "qualification-result.json"),
                                   "command_exit_code": process.returncode}
            save()
        corpus = output / "corpus-read"
        corpus.mkdir()
        instrument = ROOT / "tools/exiftool-tables/corpus_read_receipt.py"
        measurement = ROOT / f"docs/public/measurements/catalog-corpus-observed-{pin}.json"
        sys.path.insert(0, str(ROOT / "tools/ci"))
        import read_regression_gate as gate
        floor = gate.published_reads(json.loads(measurement.read_text()), pin).corpus_files
        perl = ops_root() / "toolchains/perl-5.38.2/prefix/bin/perl5.38.2"
        exiftool = bootstrap.exiftool_root(ops_root())
        # The published read snapshot and CI gate measure this exact pinned
        # ExifTool sample tree, not bootstrap's larger combined corpus.
        samples = exiftool / "t/images"
        commands = corpus_commands(corpus, instrument, perl, exiftool, samples, floor)
        for index, command in enumerate(commands):
            with (corpus / f"stage-{index}.log").open("w") as log:
                process = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
            if process.returncode:
                raise ValueError(f"corpus stage {index} exited {process.returncode}")
        result["corpus_gate"] = "PASS"
        result["corpus_receipt_sha256"] = sha(corpus / "observations" / "receipt.json")
        result["selected_file_floor"] = floor
        # Re-run the instrument's full committed-result loader after all
        # stages. Its path and binary checks require the retained Spot source,
        # input trees and targets; the downloaded receipts preserve a digest
        # binding to this on-host replay, not a claim of portable loader replay.
        loaded = {}
        for index, row in enumerate(rows):
            marker = output / f"spot-{expected_head[:12]}-{index}" / "qualification-result.json"
            committed = q.load_committed_result(marker)
            if committed["caller"]["head"] != expected_head or [r["id"] for r in committed["rows"]] != [row]:
                raise ValueError(f"Task19 {row} changed before final loader replay")
            marker_sha = sha(marker)
            if marker_sha != result["rows"][row]["marker_sha256"]:
                raise ValueError(f"Task19 {row} marker changed after row validation")
            loaded[row] = marker_sha
        result["committed_loader_replay"] = {
            "schema": 1, "kind": "oxidex_spot_task19_loader_replay", "status": "PASS",
            "head": expected_head, "pin": pin, "matrix_sha256": sha(q.CANONICAL_MATRIX),
            "target_root": str(target_root), "lease_path": str(lease), "rows": loaded}
        result["status"] = "PASS"
        save()
        return 0
    except Exception as error:
        result["status"] = "REFUSED"
        result["error"] = str(error)
        save()
        print(error, file=sys.stderr)
        return 2


def resolve_project(explicit: str | None) -> str:
    project = explicit or os.environ.get("OXIDEX_REMOTE_PROJECT")
    if not project:
        project = subprocess.check_output(["gcloud", "config", "get-value", "project"], text=True).strip()
    if not project or project == "(unset)":
        raise ValueError("set OXIDEX_REMOTE_PROJECT or configure a gcloud project")
    return project


def validate_downloaded_marker(marker: Path, output: Path, head: str,
                               row: str, run_id: str, pin: str) -> None:
    """Recheck the downloaded marker and its relocated receipt bindings.

    The remote runner first calls Task19's full load_committed_result before
    hashing the marker into its summary. Its absolute paths refer to the
    remote ops mount, so the local verifier maps them to staged bytes.
    """
    q = qualification_module()
    try:
        final = json.loads(marker.read_text())
        caller = final["caller"]
        rows = final["rows"]
        matrix = final["matrix"]
        policy = final["read_policy_input"]
        manifest = final["receipt_manifest"]
        item = rows[0]
        if (not isinstance(final, dict) or final.get("schema") != q.SCHEMA
                or final.get("kind") != q.RESULT_KIND or final.get("run_id") != run_id
                or final.get("status") != "tooling-executed-nonpromoting"
                or final.get("promotion") != "forbidden" or final.get("caller_restored") is not True
                or caller.get("head") != head or caller.get("pin_version") != pin
                or caller.get("status") != "clean" or len(rows) != 1
                or item.get("id") != row or item.get("qualification_outcome") != "pending"
                or item.get("promotion") != "forbidden" or item.get("caller_restored") is not True
                or Path(matrix["path"]).name != q.CANONICAL_MATRIX.name
                or matrix.get("sha256") != sha(q.CANONICAL_MATRIX)
                or policy.get("path") != str(output / "read-policy-input.json")
                or policy.get("sha256") != sha(marker.parent.parent / "read-policy-input.json")
                or item.get("read_policy_input") != policy):
            raise ValueError("downloaded Task19 marker has invalid committed identity")
        floors = json.loads((marker.parent.parent / "read-policy-input.json").read_text())["rows"][row]
        if item.get("read_payload_floors") != floors:
            raise ValueError("downloaded Task19 marker differs from frozen read floors")
        names = {"owner_receipt": "lease-owner.json", "heartbeat_receipt": "lease-heartbeat.jsonl",
                 "expiry_receipt": "lease-expiry.json", "release_receipt": "lease-release.json",
                 "handoff_receipt": "handoff.jsonl"}
        if set(manifest) != {*names, "row_results"} or len(manifest["row_results"]) != 1:
            raise ValueError("downloaded Task19 marker lacks its receipt manifest")
        for key, name in names.items():
            local = marker.parent / name
            if manifest[key] != {"path": str(output / run_id / name), "sha256": sha(local)}:
                raise ValueError(f"downloaded Task19 {name} receipt differs from marker")
            if name.endswith(".jsonl"):
                records = [json.loads(line) for line in local.read_text().splitlines()]
                if not records or any(record.get("run_id") != run_id or
                                      record.get("qualification_outcome") != "pending" for record in records):
                    raise ValueError(f"downloaded Task19 {name} has invalid outcome")
            else:
                receipt = json.loads(local.read_text())
                if receipt.get("run_id") != run_id or receipt.get("qualification_outcome") != "pending":
                    raise ValueError(f"downloaded Task19 {name} has invalid outcome")
        row_result = marker.parent / row / "transition-result.json"
        if (manifest["row_results"][0] !=
                {"path": str(output / run_id / row / "transition-result.json"), "sha256": sha(row_result)}
                or json.loads(row_result.read_text()) != item):
            raise ValueError("downloaded Task19 row receipt differs from marker")
        pair = marker.parent / row / "read-policy-pair.json"
        if item.get("read_policy_pair") != {"path": str(output / run_id / row / pair.name),
                                            "sha256": sha(pair)}:
            raise ValueError("downloaded Task19 read-policy pair differs from marker")
        owner = json.loads((marker.parent / "lease-owner.json").read_text())
        release = json.loads((marker.parent / "lease-release.json").read_text())
        expiry = json.loads((marker.parent / "lease-expiry.json").read_text())
        observed, deadline = final["deadline_observed_at"], final["lease_expires_at"]
        if (not isinstance(observed, (int, float)) or not isinstance(deadline, (int, float))
                or not math.isfinite(observed) or not math.isfinite(deadline) or observed >= deadline
                or owner.get("lease_expires_at") != deadline
                or expiry.get("lease_expires_at") != deadline
                or expiry.get("expiry_status") != "not-expired"
                or expiry.get("terminal_status") != "body-validated"
                or release.get("release_status") != "released"
                or release.get("terminal_status") != "body-validated"
                or release.get("flock_release_confirmed") is not True
                or release.get("surviving_children") != [] or release.get("receipt_failures") != []):
            raise ValueError("downloaded Task19 marker lacks release/deadline proof")
    except (OSError, ValueError, TypeError, KeyError, IndexError, AttributeError) as error:
        raise RuntimeError(f"downloaded Task19 marker is not committed evidence: {error}") from error


def validate_downloaded_corpus(receipt: Path, summary: dict, head: str, pin: str,
                               replay: dict, archive_sha: str) -> None:
    """Check cheap relocated bindings; full transcript/gate replay ran on Spot."""
    try:
        body = json.loads(receipt.read_text())
        files = body["corpus"]["files"]
        producer = body["producer"]
        if (not isinstance(body, dict) or body.get("schema") != "oxidex_corpus_read_receipt_v2"
                or body.get("instrument") != "corpus_read_receipt.py"
                or producer.get("source_commit") != head or producer.get("source_dirty") is not False
                or body["build_proof"]["snapshot"]["source_commit"] != head
                or body["native"]["exiftool_version"] != pin
                or not isinstance(files, dict) or len(files) != summary.get("selected_file_floor")
                or body["metric_c"]["corpus_files"] != len(files)
                or len(body["observations"]) != 2 * len(files)
                or set(body["sources"]) != set(files)
                or replay != {"schema": 1, "kind": "oxidex_remote_corpus_archive_replay",
                              "status": "PASS", "head": head, "pin": pin,
                              "archive_sha256": archive_sha,
                              "corpus_receipt_sha256": sha(receipt),
                              "corpus_files": len(files),
                              "task19_markers": {row: summary["rows"][row]["marker_sha256"]
                                                 for row in summary["rows"]}}):
            raise ValueError("corpus receipt or remote archive replay identity differs")
    except (OSError, ValueError, TypeError, KeyError, AttributeError) as error:
        raise RuntimeError(f"downloaded corpus receipt is not a replayed PASS: {error}") from error


def verify_result_tree(staged_output: Path, output: Path, head: str, pin: str,
                       corpus_replay: dict, archive_sha: str) -> None:
    """Recheck digest-bound downloaded receipts without running the payload locally."""
    if not staged_output.is_dir():
        raise RuntimeError("remote result archive lacks the qualification output root")
    summary = json.loads((staged_output / "remote-qualification.json").read_text())
    expected_rows = {row.format(pin=pin) for row in ROWS}
    if (summary.get("status") != "PASS" or summary.get("head") != head
            or summary.get("pin") != pin or summary.get("corpus_gate") != "PASS"
            or set(summary.get("rows", {})) != expected_rows):
        raise RuntimeError("remote summary is not an exact-head three-row and corpus PASS")
    preparation = staged_output / "preparation.json"
    if (not preparation.is_file() or summary.get("preparation_sha256") != sha(preparation)):
        raise RuntimeError("remote preparation binding is absent or changed")
    prepared = json.loads(preparation.read_text())
    if (prepared.get("head") != head or prepared.get("pin") != pin
            or summary.get("reference_policy_sha256") != prepared.get("policy_reference_sha256")
            or prepared.get("policy_sha256") != sha(staged_output / "read-policy-input.json")):
        raise RuntimeError("remote preparation does not bind candidate and frozen floor policy")
    input_digest = summary.get("input_archive_sha256")
    if (not isinstance(input_digest, str) or len(input_digest) != 64
            or any(char not in "0123456789abcdef" for char in input_digest)
            or type(summary.get("input_file_count")) is not int
            or summary["input_file_count"] <= 0):
        raise RuntimeError("remote summary lacks the Spot-frozen input archive identity")
    corpus_receipt = staged_output / "corpus-read" / "observations" / "receipt.json"
    if not corpus_receipt.is_file() or sha(corpus_receipt) != summary.get("corpus_receipt_sha256"):
        raise RuntimeError("downloaded corpus receipt differs from remote summary")
    validate_downloaded_corpus(corpus_receipt, summary, head, pin, corpus_replay, archive_sha)
    for index, row in enumerate(ROWS):
        run_id = f"spot-{head[:12]}-{index}"
        marker = staged_output / run_id / "qualification-result.json"
        if not marker.is_file() or sha(marker) != summary["rows"][row.format(pin=pin)]["marker_sha256"]:
            raise RuntimeError("downloaded Task19 marker differs from remote summary")
        validate_downloaded_marker(marker, output, head, row.format(pin=pin), run_id, pin)
    replayed = {row: summary["rows"][row]["marker_sha256"] for row in expected_rows}
    if summary.get("committed_loader_replay") != {
            "schema": 1, "kind": "oxidex_spot_task19_loader_replay", "status": "PASS",
            "head": head, "pin": pin, "matrix_sha256": sha(qualification_module().CANONICAL_MATRIX),
            "target_root": "/target", "lease_path": str(output / "transition.host.lock"),
            "rows": replayed}:
        raise RuntimeError("downloaded Task19 markers lack the on-Spot committed-loader replay binding")


def publish_results(staged_output: Path, output: Path, head: str, pin: str,
                    corpus_replay: dict, archive_sha: str, remote_output: Path | None = None) -> Path:
    """Verify a complete fetched result tree, then publish it in one rename."""
    verify_result_tree(staged_output, remote_output or output, head, pin, corpus_replay, archive_sha)
    if output.exists() or output.is_symlink():
        raise ValueError(f"local qualification output already exists: {output}")
    output.mkdir(parents=True)
    published = output / "remote-results"
    if published.exists() or published.is_symlink():
        raise ValueError(f"remote qualification results already exist: {published}")
    staged_output.replace(published)
    return published


def verify_archived_receipts(archive: Path, output: Path, published: Path) -> None:
    """Bind every published evidence file to its retained archive bytes."""
    prefix = str(output.relative_to(Path("/target/ops"))) + "/"
    observed = set()
    with tarfile.open(archive, "r:gz") as source:
        for member in source:
            if not member.name.startswith(prefix):
                raise RuntimeError("retained archive has a member outside published results")
            relative = member.name[len(prefix):]
            if not relative or ".." in Path(relative).parts:
                raise RuntimeError("retained archive has an unsafe published path")
            path = published / relative
            if member.isdir():
                if path.is_symlink() or not path.is_dir():
                    raise RuntimeError("published evidence directory differs from archive")
                continue
            if (not member.isfile() or relative in observed
                    or path.is_symlink() or not path.is_file()):
                raise RuntimeError("retained archive has invalid or duplicate evidence")
            stream = source.extractfile(member)
            if stream is None:
                raise RuntimeError("retained archive evidence has no bytes")
            digest = hashlib.sha256()
            size = 0
            with stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(block)
                    size += len(block)
            if size != member.size or digest.hexdigest() != sha(path):
                raise RuntimeError("published evidence differs from retained result archive")
            observed.add(relative)
    actual = set()
    for path in published.rglob("*"):
        if path.is_symlink():
            raise RuntimeError("published evidence contains a symlink")
        if path.is_file():
            actual.add(str(path.relative_to(published)))
        elif not path.is_dir():
            raise RuntimeError("published evidence contains a non-file entry")
    if observed != actual or not observed:
        raise RuntimeError("published evidence inventory differs from retained archive")


def verify_published(output: Path, transport: Path) -> dict:
    """Recheck the retained source, archive and relocated receipt bindings."""
    root = ops_root()
    if (output.resolve() != output or not output.is_dir() or not output.is_relative_to(root)
            or transport.resolve() != transport or not transport.is_file()
            or not transport.is_relative_to(root)):
        raise ValueError("published qualification paths must be real durable ops paths")
    record = json.loads(transport.read_text())
    head, pin = record.get("head"), record.get("pin")
    remote_output = record.get("remote_output")
    run_id = record.get("run_id")
    if (record.get("status") != "PASS" or not isinstance(head, str)
            or git("rev-parse", "HEAD") != head or git("status", "--porcelain")
            or pin != (ROOT / ".exiftool-version").read_text().strip()
            or not isinstance(run_id, str) or not re.fullmatch(r"qualification-[0-9a-f]{12}-[0-9a-f]{32}", run_id)
            or remote_output != f"/target/ops/evidence/remote-qualification/{run_id}"):
        raise RuntimeError("transport does not bind this exact clean source and remote output")
    result_dir = output / "remote-results"
    if result_dir.resolve() != result_dir or not result_dir.is_dir():
        raise ValueError("published result directory must be real")
    archive = transport.parent / "results.tar.gz"
    bundle = transport.parent / "repository.bundle"
    signer = transport.parent / "maintainer.allowed_signers"
    digest = record.get("results_sha256")
    if (not isinstance(digest, str) or archive.is_symlink() or bundle.is_symlink()
            or signer.is_symlink() or not signer.is_file()
            or sha(signer) != record.get("signers_sha256")
            or not archive.is_file() or sha(archive) != digest
            or not bundle.is_file() or sha(bundle) != record.get("source_bundle_sha256")):
        raise RuntimeError("retained result archive or source bundle digest differs")
    source_identity = record.get("source_identity")
    if not isinstance(source_identity, dict):
        raise RuntimeError("transport lacks source identity")
    attestation = transport.parent / "source-attestation" if source_identity.get("mode") == "github-squash-attested" else None
    verify_source_result_binding(result_dir, head, signer, bundle, attestation,
                                 run_id, source_identity)
    if source_identity["mode"] == "github-squash-attested" and record.get("host_source_admission") != "PASS":
        raise RuntimeError("host-owned source admission did not complete")
    if record.get("results_dir") != str(result_dir):
        raise RuntimeError("transport does not name the published result directory")
    target_path = f"/mnt/runner-data/remote-build/targets/{run_id}"
    source_path = f"/mnt/runner-data/remote-build/sources/{run_id}"
    expected_retained = {"status": "RETAINED_AT_PUBLICATION",
        "instance": record.get("instance"), "instance_id": record.get("instance_id"),
        "zone": record.get("zone"), "source_path": source_path,
        "target_path": target_path, "output_path": remote_output,
        "lease_path": remote_output + "/transition.host.lock",
        "input_archive_path": remote_output + "/spot-input.tar.gz",
        "input_archive_sha256": record.get("input_archive_sha256"),
        "portable_loader_replay": False}
    summary = json.loads((result_dir / "remote-qualification.json").read_text())
    if (record.get("retained_remote_recheck") != expected_retained
            or summary.get("input_archive_sha256") != record.get("input_archive_sha256")):
        raise RuntimeError("transport lacks exact retained source, target, lease or input binding")
    verify_archived_receipts(archive, Path(remote_output), result_dir)
    verify_result_tree(result_dir, Path(remote_output), head, pin,
                       record["corpus_archive_replay"], digest)
    return {"schema": 1, "kind": "oxidex_remote_published_receipt_check", "status": "PASS",
            "head": head, "archive_sha256": digest,
            "task19_loader_replay": "attested-on-Spot; remote targets required for full rerun"}


def require_remote_success(receipt: dict) -> None:
    code = receipt["remote_exit_code"]
    if code not in (0, 2):
        receipt["status"] = "RUNNING_RETAINED"
        receipt["unconfirmed_task19_exit"] = code
        raise RuntimeError("remote qualification outcome or lease state is unconfirmed; retained remote stage requires inspection")
    if code != 0:
        raise RuntimeError(f"remote qualification exited {code}")


def confirmed_exit_status(status: str, receipt: dict) -> int:
    """A malformed detached result is unknown, never an ordinary refusal."""
    if not status.isascii() or not status.isdigit() or len(status) > 3 or int(status) > 255:
        receipt["status"] = "RUNNING_RETAINED"
        raise RuntimeError("remote exit status is unconfirmed; retained stage requires inspection")
    return int(status)



def write_result_file(path: Path, result: dict, kind: str) -> None:
    """Emit one machine-readable proof independent of launcher diagnostics."""
    expected = {"pack": "pack-result.json", "replay": "replay-result.json"}[kind]
    if path != Path("/target") / expected or path.exists() or path.is_symlink():
        raise ValueError("qualification machine result path is not a new fixed target file")
    temporary = path.with_name("." + path.name + ".tmp")
    try:
        with temporary.open("x") as stream:
            json.dump(result, stream, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--project")
    parser.add_argument("--remote-run", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--verify-results-tar", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--expected-head", help=argparse.SUPPRESS)
    parser.add_argument("--verify-published", action="store_true")
    parser.add_argument("--transport", type=Path)
    parser.add_argument("--source-attestation", type=Path,
                        help="directory containing maintainer-signed GitHub squash source evidence")
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--provisioning-reference", type=Path)
    parser.add_argument("--prepare-inputs", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--pack-results", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--result-file", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.source_attestation:
        os.environ["OXIDEX_QUALIFICATION_SOURCE_ATTESTATION"] = str(args.source_attestation.expanduser().absolute())
    output = args.output.expanduser().absolute()
    if args.prepare_inputs or args.remote_run or args.verify_results_tar or args.pack_results:
        os.environ["OXIDEX_OPS_DIR"] = "/target/ops"
        os.environ["OXIDEX_TARGET_ROOT"] = "/target"
    if args.pack_results:
        if not args.expected_head:
            parser.error("--pack-results requires --expected-head")
        if not args.result_file:
            parser.error("--pack-results requires --result-file")
        write_result_file(args.result_file, pack_results(output, args.expected_head), "pack")
        return 0
    if args.prepare_inputs:
        if not args.expected_head or not args.reference:
            parser.error("--prepare-inputs requires --expected-head and --reference")
        from qualification_prepare import prepare
        print(json.dumps(prepare(args.reference, output, args.expected_head), sort_keys=True))
        return 0
    if args.remote_run:
        if not args.expected_head:
            parser.error("--remote-run requires --expected-head")
        return remote_run(output, args.expected_head)
    if args.verify_results_tar:
        if not args.expected_head:
            parser.error("--verify-results-tar requires --expected-head")
        if not args.result_file:
            parser.error("--verify-results-tar requires --result-file")
        write_result_file(args.result_file,
                          replay_archived_corpus(args.verify_results_tar, output, args.expected_head),
                          "replay")
        return 0
    if args.verify_published:
        if not args.transport:
            parser.error("--verify-published requires --transport")
        print(json.dumps(verify_published(output, args.transport.expanduser().absolute()), sort_keys=True))
        return 0
    if not args.reference or not args.provisioning_reference:
        parser.error("remote qualification requires --reference and --provisioning-reference")
    from qualification_transport import run
    return run(output, args.reference.expanduser().absolute(),
               args.provisioning_reference.expanduser().absolute(), args.project)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, ValueError, OSError, KeyError, TypeError, tarfile.TarError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f"remote qualification refused: {exc}")
