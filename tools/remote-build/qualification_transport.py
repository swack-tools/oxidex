"""Direct, identity-bound transport for restricted Task19 qualification."""
from __future__ import annotations

import hashlib
import io
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import shutil
import subprocess
import tarfile
import time
import sys
from types import SimpleNamespace

from lib.remote_build import source_sync_command, unique_run_id, verify_builder_admission
from lib.ssh_transport import DirectTransport, identity
from lib.worker_selection import select_worker
from lib.config import builder_instance_name
from qualification_source import verify_source, ATTESTED

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.release import approved_linux_perl

SOURCE_HOST = "/mnt/runner-data/remote-build/sources"
TARGET_HOST = "/mnt/runner-data/remote-build/targets"
LAUNCHER = "/usr/local/bin/oxidex-remote-build"
REFERENCE_FILES = {
    "13.59": ("capture.json", "catalog.json", "plan.json", "resolution.json",
              "materialization.json", "locations.json", "read-fixtures.json",
              "write-fixtures.json", "native-cases.json"),
    "11.78-12.64": ("capture.json", "catalog.json", "plan.json",
                    "resolution.json", "materialization.json", "locations.json",
                    "read-fixtures-11.78.json", "read-fixtures-12.64.json",
                    "write-fixtures-11.78.json", "write-fixtures-12.64.json",
                    "native-cases-11.78.json", "native-cases-12.64.json"),
}


def command(project: str, *argv: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", project):
        raise ValueError("invalid qualification launcher project")
    if any(not isinstance(arg, str) or "\x00" in arg for arg in argv):
        raise ValueError("invalid qualification launcher argument")
    return shlex.join(["sudo", "-n", LAUNCHER, project, *argv])


def host_source_admission(ssh, run_id: str, head: str, bundle_sha256: str,
                          source_identity: dict) -> bool:
    """Require the installed launcher gate before any attested Python runs."""
    if source_identity["mode"] != ATTESTED:
        return False
    ssh(command(run_id, "verify-source", "--head", head,
                "--bundle-sha256", bundle_sha256, "--run-id", run_id))
    return True


def source_archive(root: Path, reference: Path, provisioning: Path, bundle: Path, signer: Path, destination: Path,
                   attestation: Path | None = None) -> str:
    """Package only public approved references and signed source for /src."""
    approved_input = reference / "approved-linux-perl.json"
    approved_linux_perl.load(
        root / "tools/release/oracle-lock.json",
        descriptor=root / "tools/release/oracle-linux-perl-identity.json",
        envelope=approved_input, require_platform=False,
    )
    members = [(approved_input, "reference/approved-linux-perl.json"),
               (root / "rust-toolchain.toml", "rust-toolchain.toml"),
               (root / "tools/remote-build/qualification_bootstrap.py", "qualification_bootstrap.py"),
               (root / "tools/remote-build/qualification_source.py", "qualification_source.py"),
               (bundle, "repository.bundle"), (signer, "maintainer.allowed_signers"),
               (reference / "read-policy-input.json", "reference/read-policy-input.json")]
    for pair, names in REFERENCE_FILES.items():
        members.extend(((reference / "downloaded/inputs/provisioned" / pair / name
                         if name.startswith("read-fixtures") else provisioning / "provisioned" / pair / name),
                        f"reference/provisioned/{pair}/{name}") for name in names)
    if attestation is not None:
        members.extend((attestation / name, "source-attestation/" + name)
                       for name in ("document.json", "document.sig", "github-commit.json",
                                    "pgp-proof.txt", "integration-ref.json"))
    with tarfile.open(destination, "w:gz", compresslevel=3) as archive:
        for path, name in members:
            if path.is_symlink() or not path.is_file():
                raise ValueError(f"qualification transport input is not a regular file: {path}")
            if path == approved_input:
                with path.open("rb") as stream:
                    data = stream.read(approved_linux_perl.MAX_ENVELOPE_BYTES + 1)
                if len(data) > approved_linux_perl.MAX_ENVELOPE_BYTES:
                    raise ValueError("approved Linux Perl envelope exceeds transport bound")
            else:
                data = path.read_bytes()
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o644
            archive.addfile(info, io.BytesIO(data))
    return hashlib.sha256(destination.read_bytes()).hexdigest()


def download(transport: DirectTransport, remote: str, local: Path, digest: str) -> None:
    """Publish a downloaded proof only after checking the remote digest."""
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise ValueError("remote proof has no SHA-256")
    temporary = local.with_name("." + local.name + "." + secrets.token_hex(8) + ".tmp")
    try:
        subprocess.run(transport.scp(temporary, remote, download=True), check=True)
        if hashlib.sha256(temporary.read_bytes()).hexdigest() != digest:
            raise ValueError("downloaded remote proof checksum differs")
        temporary.replace(local)
    finally:
        temporary.unlink(missing_ok=True)


def downloaded_result(ssh, transport: DirectTransport, target_host: str,
                      evidence: Path, name: str) -> dict:
    """Read a result file independently of the launcher's stdout prelude."""
    if name not in {"pack-result.json", "replay-result.json"}:
        raise ValueError("unknown qualification machine result")
    remote = target_host + "/" + name
    digest = ssh("sha256sum " + shlex.quote(remote)).stdout.split()[0]
    local = evidence / name
    download(transport, remote, local, digest)
    return json.loads(local.read_text())


def qualification_worker(project: str):
    """Honor explicit pinned builders; automatic dispatch keeps its window gate."""
    name = os.environ.get("OXIDEX_REMOTE_INSTANCE")
    zone = os.environ.get("OXIDEX_REMOTE_ZONE")
    pinned_id = os.environ.get("OXIDEX_REMOTE_INSTANCE_ID")
    explicit = any(value is not None for value in (name, zone, pinned_id))
    if explicit:
        if (not all((name, zone, pinned_id)) or not builder_instance_name(name)
                or not re.fullmatch(r"[0-9]{1,20}", pinned_id)):
            raise ValueError("explicit qualification requires builder name, zone and numeric pinned ID")
        vm = SimpleNamespace(name=name, zone=zone, instance_id=pinned_id)
    else:
        vm, sample = select_worker(project)
    transport = DirectTransport(vm.name, vm.zone, project, vm.instance_id)
    if not re.fullmatch(r"[0-9]{1,20}", transport.instance_id):
        raise ValueError("qualification requires a numeric pinned VM ID")
    admission = verify_builder_admission(vm.name, vm.zone, project, vm.instance_id, transport.ssh)
    if explicit:
        sample = tuple(admission["resource_probe"][key] for key in ("cpu", "memory"))
    return vm, sample, transport


def run(output: Path, reference: Path, provisioning: Path, project: str | None) -> int:
    import qualification as q
    if identity() is None:
        raise ValueError("qualification requires pre-enrolled direct SSH identity")
    root = q.ops_root()
    if (output.exists() or output.is_symlink() or not output.is_absolute()
            or not output.is_relative_to(root)):
        raise ValueError("qualification output must be a new durable ops directory")
    if (reference.is_symlink() or not reference.is_dir() or not reference.is_relative_to(root)
            or reference.resolve() != reference):
        raise ValueError("approved reference must be a real durable ops directory")
    if (provisioning.is_symlink() or not provisioning.is_dir() or not provisioning.is_relative_to(root)
            or provisioning.resolve() != provisioning):
        raise ValueError("provisioning reference must be a real durable ops directory")
    head = q.git("rev-parse", "HEAD")
    if q.git("status", "--porcelain"):
        raise ValueError("qualification candidate must be clean")
    signer_path = Path(q.git("config", "--path", "--get", "gpg.ssh.allowedSignersFile"))
    if not signer_path.is_file():
        raise ValueError("maintainer public allowed-signers file is unavailable")
    attestation = q.source_attestation()
    try:
        timeout = int(os.environ.get("OXIDEX_REMOTE_QUALIFICATION_TIMEOUT_SECONDS", "43200"))
    except ValueError as error:
        raise ValueError("qualification observation timeout must be a positive integer") from error
    if timeout <= 0:
        raise ValueError("qualification observation timeout must be a positive integer")
    project = q.resolve_project(project)
    if attestation is not None:
        # The issuer fixes the namespace before signing; never silently
        # generate another run ID and reuse its signature.
        from qualification_source import _pairs
        document = json.loads((attestation / "document.json").read_bytes(), object_pairs_hook=_pairs)
        run_id = document.get("run_id")
    else:
        run_id = unique_run_id("qualification-" + head[:12])
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", run_id):
        raise ValueError("invalid generated qualification namespace")
    evidence = root / "evidence/remote-qualification" / run_id
    evidence.mkdir(parents=True)
    transport_path = evidence / "transport.json"
    receipt = {"schema": 1, "kind": "oxidex_remote_spot_transport", "status": "pending",
               "head": head, "pin": (q.ROOT / ".exiftool-version").read_text().strip(),
               "project": project, "run_id": run_id,
               "reference_policy_sha256": q.sha(reference / "read-policy-input.json"),
               "provisioning_reference": str(provisioning)}
    def save():
        staged = transport_path.with_name(transport_path.name + ".new")
        with staged.open("w") as stream:
            stream.write(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        staged.replace(transport_path)
        directory_fd = os.open(evidence, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    save()
    source_host = f"{SOURCE_HOST}/{run_id}"
    target_host = f"{TARGET_HOST}/{run_id}"
    remote_output = f"/target/ops/evidence/remote-qualification/{run_id}"
    bundle = evidence / "repository.bundle"
    if attestation is not None:
        bundle.write_bytes((attestation / "repository.bundle").read_bytes())
    else:
        subprocess.run(["git", "-C", str(q.ROOT), "bundle", "create", str(bundle), "HEAD"], check=True)
    subprocess.run(["git", "-C", str(q.ROOT), "bundle", "verify", str(bundle)], check=True,
                   stdout=subprocess.DEVNULL)
    signer = evidence / "maintainer.allowed_signers"
    signer.write_bytes(signer_path.read_bytes())
    source_identity = verify_source(q.ROOT, head, signer, attestation,
                                    bundle_sha256=q.sha(bundle), run_id=run_id)
    receipt["source_identity"] = source_identity
    if attestation is not None:
        retained_attestation = evidence / "source-attestation"
        shutil.copytree(attestation, retained_attestation, symlinks=False)
        attestation = retained_attestation
        verify_source(q.ROOT, head, signer, attestation,
                      bundle_sha256=q.sha(bundle), run_id=run_id)
    archive = evidence / "source.tar.gz"
    digest = source_archive(q.ROOT, reference, provisioning, bundle, signer, archive, attestation)
    if q.git("rev-parse", "HEAD") != head or q.git("status", "--porcelain"):
        raise ValueError("candidate changed while preparing qualification source")
    try:
        vm, sample, transport = qualification_worker(project)
    except Exception as error:
        receipt.update(status="BLOCKED_WORKER_SELECTION", error=str(error))
        save()
        return 2
    receipt.update({"instance": vm.name, "instance_id": vm.instance_id,
               "zone": vm.zone, "utilization": {"cpu": sample[0], "memory": sample[1]},
               "source_path": source_host, "target_path": target_host,
               "remote_output": remote_output, "source_archive_sha256": digest,
               "source_bundle_sha256": q.sha(bundle), "signers_sha256": q.sha(signer)})
    def ssh(shell: str, *, check=True):
        return subprocess.run(transport.ssh(shell), check=check, capture_output=True, text=True)
    save()
    try:
        ssh(command(run_id, "prepare"))
        upload_name = "oxidex-remote-source-" + run_id + ".tar.gz"
        subprocess.run(transport.scp(archive, "~/" + upload_name), check=True)
        ssh(source_sync_command(digest, upload_name, source_host))
        if source_identity["mode"] == ATTESTED:
            # The installed launcher must implement this host-owned admission
            # using its own trusted key before it permits uploaded Python to
            # run. Until that infrastructure contract is provisioned, this
            # command fails closed and no qualification payload starts.
            host_source_admission(ssh, run_id, head, receipt["source_bundle_sha256"],
                                  source_identity)
            receipt["host_source_admission"] = "PASS"
            save()
        boot = command(run_id, "python3", "/src/qualification_bootstrap.py", "--head", head,
                       "--output", remote_output, "--bundle-sha256", receipt["source_bundle_sha256"],
                       "--signers-sha256", receipt["signers_sha256"])
        ssh(boot)
        receipt["status"] = "prepared"
        save()
        job = command(run_id, "python3", "/target/checkout/tools/remote-build/qualification.py",
                      "--remote-run", "--output", remote_output, "--expected-head", head)
        # The host-side shell records the exact launcher exit without aborting a
        # potentially live child when the controller's observation window ends.
        script = "#!/bin/sh\nset +e\n" + job + " > " + shlex.quote(source_host + "/run.log") + \
                 " 2>&1\ncode=$?\nprintf '%s\\n' \"$code\" > " + \
                 shlex.quote(source_host + "/exit.status.tmp") + "\nmv -f " + \
                 shlex.quote(source_host + "/exit.status.tmp") + " " + \
                 shlex.quote(source_host + "/exit.status") + "\n"
        # A lost SSH acknowledgement can occur after nohup starts the child.
        # Persist every recovery handle and the uncertain state first.
        receipt["remote_job"] = {"run_script": source_host + "/run.sh",
                                  "launch_pid_file": source_host + "/launch.pid",
                                  "exit_status_file": source_host + "/exit.status",
                                  "exit_status_tmp_file": source_host + "/exit.status.tmp",
                                  "log": source_host + "/run.log"}
        receipt["status"] = "RUNNING_RETAINED"
        receipt["launch_acknowledged"] = False
        save()
        ssh("printf '%s' " + shlex.quote(script) + " > " + shlex.quote(source_host + "/run.sh") +
            " && chmod 700 " + shlex.quote(source_host + "/run.sh") +
            " && { nohup setsid sh " + shlex.quote(source_host + "/run.sh") +
            " </dev/null >/dev/null 2>&1 & printf '%s\\n' \"$!\" > " +
            shlex.quote(source_host + "/launch.pid") + "; }")
        receipt["launch_acknowledged"] = True
        receipt["status"] = "running"
        save()
        deadline = time.monotonic() + timeout
        while True:
            if time.monotonic() >= deadline:
                receipt["status"] = "RUNNING_RETAINED"
                raise RuntimeError("observation timeout; exact remote job and targets retained without retry")
            poll = ssh("if test -f " + shlex.quote(source_host + "/exit.status") +
                       "; then cat " + shlex.quote(source_host + "/exit.status") +
                       "; elif test -f " + shlex.quote(source_host + "/launch.pid") +
                       " && kill -0 \"$(cat " + shlex.quote(source_host + "/launch.pid") +
                       ")\" 2>/dev/null; then echo running; else echo lost; fi", check=False)
            if poll.returncode or poll.stdout.strip() == "lost":
                receipt["status"] = "RUNNING_RETAINED"
                raise RuntimeError("remote qualification state is unconfirmed; exact job retained")
            status = poll.stdout.strip()
            if status != "running":
                receipt["remote_exit_code"] = q.confirmed_exit_status(status, receipt)
                # A valid numeric exit is not necessarily a proven terminal
                # outcome. Task19 exits 4/5 and killed launchers retain work.
                if receipt["remote_exit_code"] not in (0, 2):
                    q.require_remote_success(receipt)
                receipt["remote_terminal_confirmed"] = True
                save()
                break
            time.sleep(min(30, max(1, deadline - time.monotonic())))
        # Preserve a remote terminal failure. Packing and downloading receipts
        # is still useful, but a failed row can never be published as PASS.
        pack = command(run_id, "python3", "/target/checkout/tools/remote-build/qualification.py",
                       "--pack-results", "--output", remote_output, "--expected-head", head,
                       "--result-file", "/target/pack-result.json")
        ssh(pack)
        remote_archive = f"{target_host}/results.tar.gz"
        packed_record = downloaded_result(ssh, transport, target_host, evidence, "pack-result.json")
        if packed_record.get("source_identity") != receipt["source_identity"]:
            raise ValueError("remote pack result source identity differs from controller")
        archive_digest = packed_record["archive_sha256"]
        download(transport, remote_archive, evidence / "results.tar.gz", archive_digest)
        staged = evidence / "extracted"
        q.safe_extract(evidence / "results.tar.gz", staged)
        staged_output = staged / "evidence/remote-qualification" / run_id
        q.verify_source_result_binding(staged_output, head, signer, bundle, attestation,
                                       run_id, receipt["source_identity"],
                                       packed_record["source_identity"])
        q.verify_archived_receipts(evidence / "results.tar.gz", Path(remote_output), staged_output)
        receipt["results_sha256"] = archive_digest
        receipt["staged_results"] = str(staged_output)
        save()
        q.require_remote_success(receipt)
        ssh(command(run_id, "python3", "/target/checkout/tools/remote-build/qualification.py",
                             "--verify-results-tar", "/target/results.tar.gz", "--output", remote_output,
                             "--expected-head", head, "--result-file", "/target/replay-result.json"))
        replay_record = downloaded_result(ssh, transport, target_host, evidence, "replay-result.json")
        if replay_record.get("archive_sha256") != archive_digest:
            raise ValueError("remote corpus replay used different archive bytes")
        receipt["corpus_archive_replay"] = replay_record
        summary = json.loads((staged_output / "remote-qualification.json").read_text())
        receipt["input_archive_sha256"] = summary["input_archive_sha256"]
        preparation = json.loads((staged_output / "preparation.json").read_text())
        if (preparation.get("head") != head or preparation.get("pin") != receipt["pin"]
                or preparation.get("policy_reference_sha256") != receipt["reference_policy_sha256"]
                or preparation.get("policy_sha256") != receipt["reference_policy_sha256"]):
            raise ValueError("remote preparation does not bind the approved floor policy and candidate")
        retained_output = target_host + "/ops/evidence/remote-qualification/" + run_id
        retained_input = retained_output + "/spot-input.tar.gz"
        check = ssh("test -d " + shlex.quote(source_host) + " && test -d " +
                    shlex.quote(target_host + "/checkout") + " && test -f " +
                    shlex.quote(retained_output + "/transition.host.lock") +
                    " && sha256sum " + shlex.quote(retained_input))
        lines = check.stdout.strip().split()
        if not lines or lines[0] != receipt["input_archive_sha256"]:
            raise ValueError("retained remote source, target, lease or frozen inputs differ")
        # Re-resolve the provider identity immediately before publication.
        DirectTransport(vm.name, vm.zone, project, vm.instance_id)
        receipt["retained_remote_recheck"] = {"status": "RETAINED_AT_PUBLICATION",
            "instance": vm.name, "instance_id": vm.instance_id, "zone": vm.zone,
            "source_path": source_host, "target_path": target_host,
            "output_path": remote_output, "lease_path": remote_output + "/transition.host.lock",
            "input_archive_path": remote_output + "/spot-input.tar.gz",
            "input_archive_sha256": receipt["input_archive_sha256"], "portable_loader_replay": False}
        receipt["results_dir"] = str(q.publish_results(staged_output, output, head, receipt["pin"],
                                                        replay_record, archive_digest, Path(remote_output)))
        receipt["status"] = "PASS"
        save()
        return 0
    except Exception as error:
        if receipt["status"] != "RUNNING_RETAINED" and receipt.get("remote_job") and not receipt.get("remote_terminal_confirmed"):
            receipt["status"] = "RUNNING_RETAINED"
        elif receipt["status"] != "RUNNING_RETAINED":
            receipt["status"] = "REFUSED"
        receipt["error"] = str(error)
        save()
        return 2
