#!/usr/bin/env python3
"""Run exact-HEAD Task19 and the corpus read gate on one Linux amd64 Spot VM.

The caller must supply an already-provisioned Task19 output root whose plan
is bound to this clean commit. This script transfers those immutable inputs,
performs all expensive work remotely, and downloads hash-checked receipts.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shlex
import subprocess
import sys
import tarfile
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from ops_paths import ops_root  # noqa: E402
from lib.worker_selection import select_worker  # noqa: E402

ROWS = ("same-pin-{pin}", "11.78-to-12.64", "12.64-to-11.78")
SSH_FLAGS = ("--ssh-flag=-oServerAliveInterval=15", "--ssh-flag=-oServerAliveCountMax=3")
SCP_FLAGS = ("--scp-flag=-oServerAliveInterval=15", "--scp-flag=-oServerAliveCountMax=3")


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
    """Use the qualification instrument itself to reject stale or incomplete inputs."""
    q = qualification_module()
    if output.is_symlink() or not output.is_dir() or not output.is_relative_to(ops_root()):
        raise ValueError("output must be a real directory beneath OXIDEX_OPS_DIR")
    caller = q.snapshot_caller(ROOT)
    head = git("rev-parse", "HEAD")
    if caller["head"] != head:
        raise ValueError("caller HEAD changed during qualification input preflight")
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


def input_paths(seeds: list[Path], root: Path) -> list[Path]:
    """Include the prepared bundle plus every ops-root file path it names."""
    pending = seeds[:]
    selected: set[Path] = set()
    while pending:
        raw = pending.pop()
        if raw.is_symlink():
            raise ValueError(f"qualification input symlink is forbidden: {raw}")
        path = raw.resolve()
        if not path.is_relative_to(root) or path in selected:
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
            def visit(value):
                if isinstance(value, str) and value.startswith(str(root) + "/"):
                    dependency = Path(value)
                    if dependency.exists():
                        pending.append(dependency)
                elif isinstance(value, dict):
                    for item in value.values():
                        visit(item)
                elif isinstance(value, list):
                    for item in value:
                        visit(item)
            visit(document)
    return sorted(path for path in selected if path.is_file())


def prepare_archive(output: Path, destination: Path, bundle: Path) -> tuple[str, str]:
    root = ops_root()
    head, pin, seeds = exact_inputs(output)
    with tarfile.open(destination, "w:gz", compresslevel=3) as archive:
        archive.add(bundle, arcname="repository.bundle", recursive=False)
        for path in input_paths(seeds, root):
            archive.add(path, arcname=str(Path("ops") / path.relative_to(root)), recursive=False)
    return head, pin


def safe_extract(archive: Path, destination: Path) -> None:
    with tarfile.open(archive, "r:gz") as source:
        members = source.getmembers()
        if any(member.issym() or member.islnk() or member.name.startswith("/") or ".." in Path(member.name).parts
               for member in members):
            raise ValueError("remote receipt archive contains an unsafe member")
        for member in members:
            target = destination / member.name
            if member.isfile() and target.exists():
                raise ValueError(f"remote receipt would replace existing evidence: {target}")
        source.extractall(destination, filter="data")


def remote_run(output: Path, expected_head: str) -> int:
    """Container phase; source and ops root are already mounted at exact paths."""
    if git("rev-parse", "HEAD") != expected_head or git("status", "--porcelain"):
        raise ValueError("remote Git checkout is not the exact clean candidate")
    # The signing key is generated on this isolated VM and used only for the
    # owned measurement checkouts. It is never transferred back or published.
    target_root = Path(os.environ.get("OXIDEX_TARGET_ROOT", str(ops_root() / "targets")))
    signing = target_root / "remote-signing"
    signing.mkdir(parents=True, exist_ok=False)
    key = signing / "id_ed25519"
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)], check=True)
    allowed = signing / "allowed_signers"
    allowed.write_text("spot-qualification@local " + (signing / "id_ed25519.pub").read_text())
    for name, value in {"user.name": "OxiDex Spot qualification", "user.email": "spot-qualification@local",
                        "user.signingkey": str(key), "gpg.format": "ssh",
                        "gpg.ssh.allowedSignersFile": str(allowed)}.items():
        subprocess.run(["git", "config", "--local", name, value], check=True)
    q = qualification_module()
    pin = (ROOT / ".exiftool-version").read_text().strip()
    rows = tuple(row.format(pin=pin) for row in ROWS)
    policy = output / "read-policy-input.json"
    result = {"schema": 1, "kind": "oxidex_remote_spot_qualification", "head": expected_head,
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
        bootstrap.provision(ops_root())
        exact_inputs(output)
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
            if process.returncode:
                raise ValueError(f"Task19 {row} exit {process.returncode}; see {run_id}.log")
            committed = q.load_committed_result(receipt / "qualification-result.json")
            if committed["caller"]["head"] != expected_head or [r["id"] for r in committed["rows"]] != [row]:
                raise ValueError(f"Task19 {row} final marker does not bind exact head and row")
            result["rows"][row] = {"run_id": run_id, "marker_sha256": sha(receipt / "qualification-result.json")}
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
        samples = bootstrap.corpus_path(ops_root())
        commands = [
            [sys.executable, str(instrument), "build", "--output", str(corpus / "build.json")],
            [sys.executable, str(instrument), "observe", "--build-proof", str(corpus / "build.json"),
             "--perl", str(perl), "--exiftool-dir", str(exiftool), "--corpus", str(samples),
             "--output", str(corpus / "receipt.json"), "--min-files", str(floor)],
            [sys.executable, str(instrument), "verify", "--receipt", str(corpus / "receipt.json")],
            [sys.executable, str(ROOT / "tools/ci/read_regression_gate.py"),
             "--receipt", str(corpus / "receipt.json")],
        ]
        for index, command in enumerate(commands):
            with (corpus / f"stage-{index}.log").open("w") as log:
                process = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
            if process.returncode:
                raise ValueError(f"corpus stage {index} exited {process.returncode}")
        result["corpus_gate"] = "PASS"
        result["corpus_receipt_sha256"] = sha(corpus / "receipt.json")
        result["selected_file_floor"] = floor
        result["status"] = "PASS"
        save()
        return 0
    except Exception as error:
        result["status"] = "REFUSED"
        result["error"] = str(error)
        save()
        print(error, file=sys.stderr)
        return 2


def controller(output: Path, project: str | None) -> int:
    head, _pin, _seeds = exact_inputs(output)
    evidence = ops_root() / "evidence" / "remote-qualification" / (head[:12] + "-" + secrets.token_hex(8))
    evidence.mkdir(parents=True)
    bundle = evidence / "repository.bundle"
    subprocess.run(["git", "-C", str(ROOT), "bundle", "create", str(bundle), "HEAD"], check=True)
    subprocess.run(["git", "-C", str(ROOT), "bundle", "verify", str(bundle)], check=True,
                   stdout=subprocess.DEVNULL)
    package = evidence / "input.tar.gz"
    frozen_head, pin = prepare_archive(output, package, bundle)
    if frozen_head != head or git("rev-parse", "HEAD") != head or git("status", "--porcelain"):
        raise ValueError("local candidate changed while freezing qualification inputs")
    project = project or subprocess.check_output(["gcloud", "config", "get-value", "project"], text=True).strip()
    if not project or project == "(unset)":
        raise ValueError("set OXIDEX_REMOTE_PROJECT or configure a gcloud project")
    vm, sample = select_worker(project)
    stage = f"/mnt/runner-data/remote-qualification/{evidence.name}"
    receipt = {"schema": 1, "kind": "oxidex_remote_spot_transport", "head": head, "pin": pin,
               "instance": vm.name, "instance_id": vm.instance_id, "zone": vm.zone,
               "utilization": {"cpu": sample[0], "memory": sample[1]}, "archive_sha256": sha(package),
               "stage": stage, "status": "pending"}
    transport = evidence / "transport.json"
    def save():
        transport.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    def ssh(command: str, *, check=True):
        return subprocess.run(["gcloud", "compute", "ssh", vm.name, f"--zone={vm.zone}",
                               f"--project={project}", "--quiet", *SSH_FLAGS,
                               "--command=" + command], check=check, capture_output=True, text=True)
    save()
    upload = f"~/oxidex-qualification-{evidence.name}.tar.gz"
    try:
        subprocess.run(["gcloud", "compute", "scp", str(package), f"{vm.name}:{upload}",
                        f"--zone={vm.zone}", f"--project={project}", "--quiet", *SCP_FLAGS], check=True)
        # All shell operands are generated safe identifiers or shlex quoted.
        setup = (f"sudo install -d -m 0770 -o \"$(id -u)\" -g \"$(id -g)\" {shlex.quote(stage)} && "
                 f"printf '%s  %s\\n' {shlex.quote(receipt['archive_sha256'])} {shlex.quote(upload[2:])} "
                 "| sha256sum -c - && "
                 f"tar -xzf {shlex.quote(upload[2:])} -C {shlex.quote(stage)} && "
                 f"git clone -q {shlex.quote(stage + '/repository.bundle')} {shlex.quote(stage + '/source')} && "
                 f"git -C {shlex.quote(stage + '/source')} checkout -q --detach {head} && "
                 f"test \"$(git -C {shlex.quote(stage + '/source')} rev-parse HEAD)\" = {head} && "
                 f"mkdir -p {shlex.quote(stage + '/targets')} && "
                 f"sudo chown -R 1001:1001 {shlex.quote(stage)}")
        ssh(setup)
        ops_mount = str(ops_root())
        if " " in ops_mount or "'" in ops_mount:
            raise ValueError("ops root has unsupported Docker mount characters")
        docker = ["sudo", "docker", "run", "--rm", "--user=1001:1001", "--cpus=" + str(vm.cpus),
                  "--memory=" + str(int(vm.memory_gib)) + "g", "--memory-swap=" + str(int(vm.memory_gib)) + "g",
                  "--pids-limit=4096", "--cap-drop=ALL", "--security-opt=no-new-privileges",
                  "-e", "OXIDEX_OPS_DIR=" + ops_mount, "-e", "OXIDEX_TARGET_ROOT=/target",
                  "-e", "CARGO_HOME=/cargo", "-e", "CARGO_TARGET_DIR=/target/corpus",
                  "-v", stage + "/source:/src", "-v", stage + "/ops:" + ops_mount,
                  "-v", stage + "/targets:/target", "-v", "/mnt/runner-data/remote-build/cargo:/cargo",
                  "-w", "/src", "oxidex-remote-builder:1.97.1", "python3",
                  "tools/remote-build/qualification.py", "--remote-run", "--output", str(output),
                  "--expected-head", head]
        run_script = "#!/bin/sh\nset +e\n" + shlex.join(docker) + f" > {shlex.quote(stage + '/run.log')} 2>&1\n" + \
                     f"printf '%s\\n' \"$?\" > {shlex.quote(stage + '/exit.status')}\n"
        ssh(f"printf '%s' {shlex.quote(run_script)} > {shlex.quote(stage + '/run.sh')} && "
            f"chmod 700 {shlex.quote(stage + '/run.sh')} && "
            f"nohup setsid sh {shlex.quote(stage + '/run.sh')} </dev/null >/dev/null 2>&1 &")
        receipt["status"] = "running"
        save()
        while True:
            poll = ssh(f"if test -f {shlex.quote(stage + '/exit.status')}; then cat {shlex.quote(stage + '/exit.status')}; "
                       "else echo running; fi", check=False)
            if poll.returncode:
                raise RuntimeError("Spot worker unreachable; retained remote stage may be resumable")
            status = poll.stdout.strip()
            if status != "running":
                receipt["remote_exit_code"] = int(status)
                break
            time.sleep(30)
        # Preserve the outer container log even if Task19 stops before a row exists.
        subprocess.run(["gcloud", "compute", "scp", f"{vm.name}:{stage}/run.log",
                        str(evidence / "remote-run.log"), f"--zone={vm.zone}",
                        f"--project={project}", "--quiet", *SCP_FLAGS], check=False)
        # Results are copied even on failure; they never imply PASS by themselves.
        pack = f"tar -czf {shlex.quote(stage + '/results.tar.gz')} -C {shlex.quote(stage + '/ops')} " + \
               " ".join(shlex.quote(str(output.relative_to(ops_root()) / name)) for name in
                        ["remote-qualification.json", "corpus-read", *[f"spot-{head[:12]}-{i}" for i in range(3)]])
        ssh(pack, check=False)
        remote_digest = ssh(f"sha256sum {shlex.quote(stage + '/results.tar.gz')}", check=False)
        if remote_digest.returncode == 0:
            digest = remote_digest.stdout.split()[0]
            result_tar = evidence / "results.tar.gz"
            subprocess.run(["gcloud", "compute", "scp", f"{vm.name}:{stage}/results.tar.gz",
                            str(result_tar), f"--zone={vm.zone}", f"--project={project}",
                            "--quiet", *SCP_FLAGS], check=True)
            if sha(result_tar) != digest:
                raise RuntimeError("remote result archive checksum differs after download")
            safe_extract(result_tar, ops_root())
            receipt["results_sha256"] = digest
        if receipt["remote_exit_code"] != 0:
            raise RuntimeError(f"remote qualification exited {receipt['remote_exit_code']}")
        summary = json.loads((output / "remote-qualification.json").read_text())
        if summary.get("status") != "PASS" or summary.get("head") != head or len(summary.get("rows", {})) != 3:
            raise RuntimeError("remote summary is not an exact-head three-row PASS")
        for index, row in enumerate(ROWS):
            marker = output / f"spot-{head[:12]}-{index}" / "qualification-result.json"
            if not marker.is_file() or sha(marker) != summary["rows"][row.format(pin=pin)]["marker_sha256"]:
                raise RuntimeError("downloaded Task19 marker differs from remote summary")
        receipt["status"] = "PASS"
        save()
        return 0
    except Exception as error:
        receipt["status"] = "REFUSED"
        receipt["error"] = str(error)
        save()
        print(f"remote qualification refused: {error}; evidence: {evidence}", file=sys.stderr)
        return 2


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--project")
    parser.add_argument("--remote-run", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--expected-head", help=argparse.SUPPRESS)
    args = parser.parse_args()
    output = args.output.expanduser().resolve()
    if args.remote_run:
        if not args.expected_head:
            parser.error("--remote-run requires --expected-head")
        return remote_run(output, args.expected_head)
    return controller(output, args.project)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f"remote qualification refused: {exc}")
