#!/usr/bin/env python3
"""Provision and authenticate the durable ExifTool release oracle.

Every mutable operation stages below the destination's durable filesystem and
renames only after verification.  Existing verified installations remain
usable if a download, extraction, build, or probe is interrupted.
"""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from collections.abc import Callable, Mapping
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator


DURABLE_ROOT = Path("/Users/allen/oxidex-ops")
MIN_CORPUS_FILES = 4_000
VERSION = "13.59"
LOCK_PATH = Path(__file__).with_name("oracle-lock.json")
LOCK = json.loads(LOCK_PATH.read_text(encoding="utf-8"))


class Refused(RuntimeError):
    """The requested operation failed a release-safety precondition."""


def resolve_durable_root(root: Path) -> Path:
    reject_symlink_components(root)
    value = root.expanduser().resolve()
    expected = DURABLE_ROOT.expanduser().resolve()
    if value != expected:
        raise Refused(f"durable root must be {expected}, not {value}")
    return value


def require_descendant(path: Path, root: Path = DURABLE_ROOT) -> Path:
    reject_symlink_components(path)
    durable = root.expanduser().resolve()
    value = path.expanduser().resolve(strict=False)
    try:
        value.relative_to(durable)
    except ValueError as exc:
        raise Refused(f"path outside durable root: {value}") from exc
    return value


def reject_symlink_components(path: Path) -> None:
    """Refuse a path containing any existing symlink component."""
    lexical = Path(os.path.abspath(path.expanduser()))
    current = Path(lexical.anchor)
    for part in lexical.parts[1:]:
        current /= part
        if current.is_symlink():
            raise Refused(
                "symlink path is refused because it may resolve outside durable root: "
                f"{current}"
            )


def resolve_durable_override(path: Path, root: Path = DURABLE_ROOT) -> Path:
    """Resolve a release-tool override and require durable storage."""
    value = require_descendant(path, root)
    if value == root.expanduser().resolve():
        raise Refused(f"override must name a path below durable root: {value}")
    return value


def perl_prefix(root: Path = DURABLE_ROOT) -> Path:
    return root / "toolchains/perl-5.38.2/prefix"


def perl_path(root: Path = DURABLE_ROOT) -> Path:
    return perl_prefix(root) / "bin/perl5.38.2"


def exiftool_root(root: Path = DURABLE_ROOT) -> Path:
    """Pinned checkout beneath the versioned cache directory.

    ``scripts.exiftool_oracle.cache_dir`` predates this bootstrap and exposes
    the versioned cache directory to callers, which append ``exiftool`` to
    reach the source checkout.  Keep that public contract while retaining the
    version boundary required for durable repair.
    """
    return root / f"cache/exiftool/{VERSION}/exiftool"


def exiftool_path(root: Path = DURABLE_ROOT) -> Path:
    return exiftool_root(root) / "exiftool"


def corpus_path(root: Path = DURABLE_ROOT) -> Path:
    """Return the public sibling corpus consumed by legacy cache callers."""
    return root / f"cache/exiftool/{VERSION}/combined-samples"


def exiftool_library_path(root: Path = DURABLE_ROOT) -> Path:
    return exiftool_root(root) / "lib/Image/ExifTool.pm"


def archive_zip_library_path(root: Path = DURABLE_ROOT) -> Path:
    candidates = sorted(
        (
            value
            for value in perl_prefix(root).glob("lib/**/Archive/Zip.pm")
            if value.is_file() and not value.is_symlink()
        ),
        key=lambda value: value.as_posix(),
    )
    if len(candidates) != 1:
        raise Refused(
            "durable Perl must contain exactly one Archive/Zip.pm, got "
            f"{len(candidates)}"
        )
    return candidates[0]


def evidence_path(root: Path = DURABLE_ROOT) -> Path:
    return (
        root
        / "evidence/20260919-beta1-functional/durable-controller-oracle-bootstrap"
    )


def manifest_path(root: Path = DURABLE_ROOT) -> Path:
    return evidence_path(root) / "storage-manifest.json"


def load_lock(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_tree(path: Path) -> str:
    """Hash file names, modes, symlink targets, and contents deterministically."""
    digest = hashlib.sha256()
    for item in sorted(path.rglob("*"), key=lambda value: value.as_posix()):
        relative = item.relative_to(path).as_posix().encode()
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(oct(item.lstat().st_mode & 0o777).encode())
        if item.is_symlink():
            digest.update(b"L")
            digest.update(os.readlink(item).encode())
        elif item.is_file():
            digest.update(b"F")
            digest.update(bytes.fromhex(sha256_file(item)))
        elif item.is_dir():
            digest.update(b"D")
    return digest.hexdigest()


def atomic_json(path: Path, value: Mapping[str, Any]) -> None:
    require_descendant(path, DURABLE_ROOT)
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent, prefix=f".{path.name}.tmp-"
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def _fsync_directory(path: Path) -> None:
    """Persist a directory entry change before reporting publication complete."""
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _perl_literal(value: str) -> str:
    """Return a Perl string expression that preserves every argument byte."""
    return "pack(q(H*),q(" + value.encode("utf-8").hex() + "))"


def write_comparison_wrapper(
    target: Path,
    perl: Path,
    library: Path,
    script: Path,
    marker_directory: Path,
) -> None:
    """Atomically publish a private wrapper that execs the authenticated Perl.

    The marker is intentionally retained outside the disposable wrapper work
    directory so the successful measurement proves the interpreter and argv
    actually used by ``tag-comparison``.
    """
    for value in (target, perl, library, script, marker_directory):
        require_descendant(value)
    if not perl.is_file() or not library.is_dir() or not script.is_file():
        raise Refused("comparison wrapper requires existing Perl, library, and script")
    target.parent.mkdir(parents=True, exist_ok=True)
    marker_directory.mkdir(parents=True, exist_ok=True)
    os.chmod(marker_directory, 0o700)
    require_descendant(target)
    require_descendant(marker_directory)
    body = (
        f"#!{perl}\n"
        "use strict; use warnings; use Fcntl qw(:DEFAULT); use IO::Handle; "
        "use JSON::PP qw(encode_json);\n"
        f"my $perl={_perl_literal(str(perl))}; "
        f"my $lib={_perl_literal(str(library))}; "
        f"my $script={_perl_literal(str(script))}; "
        f"my $markers={_perl_literal(str(marker_directory))};\n"
        "my $marker=$markers . q(/perl-) . $$ . q(.json); "
        "my $temporary=$marker . q(.new); "
        "sysopen(my $out,$temporary,O_WRONLY|O_CREAT|O_EXCL,0600) "
        "or die qq(marker create failed: $!\\n); "
        "chmod(0600,$temporary) or die qq(marker chmod failed: $!\\n); "
        "print {$out} encode_json({schema_version=>1,marker=>q(COMPARE_EXIFTOOL_PERL),"
        "perl=>$perl,exiftool_lib=>$lib,exiftool_script=>$script,argv=>\\@ARGV}) "
        "or die qq(marker write failed: $!\\n); "
        "$out->sync() or die qq(marker fsync failed: $!\\n); "
        "close($out) or die qq(marker close failed: $!\\n); "
        "rename($temporary,$marker) or die qq(marker publish failed: $!\\n); "
        "sysopen(my $marker_parent,$markers,O_RDONLY) "
        "or die qq(marker parent open failed: $!\\n); "
        "$marker_parent->sync() or die qq(marker parent fsync failed: $!\\n); "
        "close($marker_parent) or die qq(marker parent close failed: $!\\n); "
        "exec {$perl} $perl, q(-I) . $lib, $script, @ARGV "
        "or die qq(exec failed: $!\\n);\n"
    )
    descriptor, temporary_name = tempfile.mkstemp(
        dir=target.parent, prefix=f".{target.name}.tmp-"
    )
    temporary = Path(temporary_name)
    try:
        os.fchmod(descriptor, 0o700)
        with os.fdopen(descriptor, "wb") as output:
            descriptor = -1
            output.write(body.encode("utf-8"))
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, target)
        _fsync_directory(target.parent)
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        temporary.unlink(missing_ok=True)


def cleanup_comparison_workdir(work_directory: Path, receipt_root: Path) -> None:
    """Remove one private wrapper directory and durably publish that removal."""
    require_descendant(work_directory)
    require_descendant(receipt_root)
    if work_directory.exists():
        shutil.rmtree(work_directory)
    _fsync_directory(receipt_root)


def validate_lock(lock: Mapping[str, Any]) -> None:
    for name in ("perl", "archive_zip"):
        item = lock.get(name)
        if not isinstance(item, Mapping) or len(str(item.get("sha256", ""))) != 64:
            raise Refused(f"{name} lock missing SHA-256 hash")
    exiftool = lock.get("exiftool")
    if not isinstance(exiftool, Mapping):
        raise Refused("ExifTool lock missing")
    tag_object = str(exiftool.get("tag_object", ""))
    if tag_object != "2200871d9cef988051d2a99d67df3bda6cbb30a8":
        raise Refused(f"wrong ExifTool tag object: {tag_object or 'missing'}")
    corpus_hash = str(lock.get("corpus_tree_sha256", ""))
    if len(corpus_hash) != 64:
        raise Refused("corpus lock missing SHA-256 hash")
    archives = lock.get("archives")
    if not isinstance(archives, Mapping) or not archives:
        raise Refused("archive locks missing")
    for name, item in archives.items():
        if not isinstance(item, Mapping):
            raise Refused(f"archive lock {name} is invalid")
        for field in ("filename", "url", "sha256"):
            if not item.get(field):
                raise Refused(f"archive lock {name} missing {field}")
        if len(str(item["sha256"])) != 64:
            raise Refused(f"archive lock {name} has invalid hash")


def _fetch_url(url: str, destination: Path) -> None:
    with urllib.request.urlopen(url, timeout=120) as response, destination.open(
        "wb"
    ) as output:
        shutil.copyfileobj(response, output)


def download_locked(
    root: Path,
    name: str,
    item: Mapping[str, str],
    *,
    fetch: Callable[[str, Path], None] = _fetch_url,
) -> Path:
    destination = require_descendant(root / "cache/downloads" / item["filename"], root)
    destination.parent.mkdir(parents=True, exist_ok=True)
    expected = item["sha256"]
    if destination.is_file() and sha256_file(destination) == expected:
        return destination

    partial = destination.with_suffix(destination.suffix + ".partial")
    partial.unlink(missing_ok=True)
    try:
        fetch(item["url"], partial)
        actual = sha256_file(partial)
        if actual != expected:
            raise Refused(
                f"locked archive hash mismatch for {name}: expected {expected}, got {actual}"
            )
        os.replace(partial, destination)
    except Refused:
        partial.unlink(missing_ok=True)
        raise
    except Exception as exc:
        partial.unlink(missing_ok=True)
        raise Refused(f"download failed for {name}: {exc}") from exc
    return destination


def _staging_pid(path: Path, destination: Path) -> int | None:
    prefix = f".{destination.name}.staging-"
    if not path.name.startswith(prefix):
        return None
    value = path.name[len(prefix) :].split("-", 1)[0]
    return int(value) if value.isdecimal() else None


def _process_is_live(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def staging_intent_path(staging: Path) -> Path:
    """Return the durable sidecar that identifies one candidate transaction."""
    return staging.with_name(f"{staging.name}.intent.json")


def _atomic_json(path: Path, value: Mapping[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.tmp-")
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as output:
            json.dump(value, output, indent=2, sort_keys=True)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def write_staging_intent(
    staging: Path,
    destination: Path,
    owner_pid: int,
    lifecycle: str,
    candidate_sha256: str | None = None,
) -> Path:
    """Persist the state required to recover a dead staging owner safely."""
    if lifecycle not in {"materializing", "verified"}:
        raise Refused(f"invalid staging lifecycle: {lifecycle}")
    path = staging_intent_path(staging)
    value: dict[str, Any] = {
        "schema_version": 1,
        "staging": str(staging),
        "destination": str(destination),
        "owner_pid": owner_pid,
        "lifecycle": lifecycle,
    }
    if lifecycle == "verified":
        if not candidate_sha256:
            raise Refused("verified staging intent needs a candidate hash")
        value["candidate_sha256"] = candidate_sha256
    _atomic_json(path, value)
    return path


def _read_staging_intent(staging: Path, destination: Path, root: Path) -> dict[str, Any]:
    sidecar = staging_intent_path(staging)
    require_descendant(sidecar, root)
    if sidecar.is_symlink() or not sidecar.is_file():
        raise Refused(f"unowned abandoned staging state requires explicit repair: {staging}")
    try:
        value = json.loads(sidecar.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise Refused(f"malformed staging intent requires explicit repair: {staging}") from exc
    pid = _staging_pid(staging, destination)
    if not isinstance(value, Mapping) or (
        value.get("schema_version") != 1
        or value.get("staging") != str(staging)
        or value.get("destination") != str(destination)
        or value.get("owner_pid") != pid
        or value.get("lifecycle") not in {"materializing", "verified"}
    ):
        raise Refused(f"ambiguous staging state requires explicit repair: {staging}")
    if value["lifecycle"] == "verified" and not isinstance(value.get("candidate_sha256"), str):
        raise Refused(f"ambiguous verified staging state: {staging}")
    return dict(value)


_STAGING_JOURNAL_EVENTS = frozenset({
    "removed-unverified-staging",
    "quarantined-redundant-authenticated-staging",
    "published-authenticated-staging",
    "preserved-verified-staging-after-caught-error",
    "removed-unverified-staging-after-caught-error",
    "preserved-ambiguous-staging-after-caught-error",
})


def staging_recovery_journal_path(destination: Path) -> Path:
    """Return the one lifecycle-metadata path excluded from staging candidates."""
    return destination.parent / f".{destination.name}.staging-recovery.jsonl"


def _validate_staging_recovery_journal(journal: Path, root: Path) -> None:
    """Accept only an exact, durable JSONL lifecycle journal as metadata."""
    require_descendant(journal, root)
    if journal.is_symlink() or not journal.is_file():
        raise Refused(f"unsafe staging recovery journal: {journal}")
    try:
        records = journal.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        raise Refused(f"malformed staging recovery journal: {journal}") from exc
    if not records:
        raise Refused(f"malformed staging recovery journal: {journal}")
    for line in records:
        try:
            record = json.loads(line)
        except json.JSONDecodeError as exc:
            raise Refused(f"malformed staging recovery journal: {journal}") from exc
        if (
            not isinstance(record, Mapping)
            or record.get("event") not in _STAGING_JOURNAL_EVENTS
            or not isinstance(record.get("staging"), str)
        ):
            raise Refused(f"malformed staging recovery journal: {journal}")
        require_descendant(Path(record["staging"]), root)
        if "quarantine" in record:
            if not isinstance(record["quarantine"], str):
                raise Refused(f"malformed staging recovery journal: {journal}")
            require_descendant(Path(record["quarantine"]), root)


def _append_staging_journal(destination: Path, event: Mapping[str, Any]) -> None:
    journal = staging_recovery_journal_path(destination)
    with journal.open("a", encoding="utf-8") as output:
        output.write(json.dumps(dict(event), sort_keys=True) + "\n")
        output.flush()
        os.fsync(output.fileno())


@contextmanager
def _staging_lifecycle_lock(root: Path, destination: Path) -> Iterator[None]:
    """Serialize staging ownership publication with recovery observation.

    The lock covers the small create-or-observe transaction only: recovery
    cannot see a newly-created staging directory until its intent sidecar is
    durable, while independent materialization still proceeds concurrently.
    """
    lock_path = destination.parent / f".{destination.name}.staging.lifecycle.lock"
    require_descendant(lock_path, root)
    with lock_path.open("a+", encoding="utf-8") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def _recover_abandoned_staging(
    root: Path, destination: Path, verify_candidate: Callable[[Path], None]
) -> Path | None:
    """Return exactly one dead, lock-authenticated candidate or fail closed.

    A named dead directory alone is not ownership.  The caller holds the
    staging lifecycle lock while it observes the directory/sidecar pair.  The
    sidecar is written
    before materialization and upgraded only after the locked verifier passes;
    therefore an authenticated tree is either published or preserved, never
    silently removed.
    """
    recovered: list[Path] = []
    journal = staging_recovery_journal_path(destination)
    for staging in destination.parent.glob(f".{destination.name}.staging-*"):
        if staging.name.endswith(".intent.json"):
            continue
        if staging == journal:
            _validate_staging_recovery_journal(journal, root)
            continue
        require_descendant(staging, root)
        if staging.is_symlink() or not staging.is_dir():
            raise Refused(f"unsafe abandoned staging state: {staging}")
        intent = _read_staging_intent(staging, destination, root)
        if _process_is_live(int(intent["owner_pid"])):
            continue
        try:
            verify_candidate(staging)
        except Refused:
            # This is the sole automatic-delete case: current locked
            # verification positively proves that a dead, owned candidate is
            # not an authenticated tree.
            _append_staging_journal(destination, {
                "event": "removed-unverified-staging", "staging": str(staging),
                "owner_pid": intent["owner_pid"],
            })
            shutil.rmtree(staging)
            staging_intent_path(staging).unlink(missing_ok=True)
            continue
        if intent["lifecycle"] != "verified" or intent["candidate_sha256"] != sha256_tree(staging):
            raise Refused(f"ambiguous authenticated staging state: {staging}")
        recovered.append(staging)
    if len(recovered) > 1:
        raise Refused("multiple authenticated staging candidates require explicit repair")
    return recovered[0] if recovered else None


def _publish_or_quarantine_recovered_staging(
    root: Path,
    destination: Path,
    staging: Path,
    verify_candidate: Callable[[Path], None],
) -> str:
    lock_path = destination.parent / f".{destination.name}.install.lock"
    require_descendant(lock_path, root)
    with lock_path.open("a+", encoding="utf-8") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            if destination.exists():
                if destination.is_symlink() or not destination.is_dir():
                    raise Refused(f"canonical destination is not a safe tree: {destination}")
                verify_candidate(destination)
                quarantine = root / "evidence/bootstrap-staging-quarantine" / destination.name / staging.name
                quarantine.parent.mkdir(parents=True, exist_ok=True)
                if quarantine.exists():
                    raise Refused(f"staging quarantine collision: {quarantine}")
                os.rename(staging, quarantine)
                os.rename(staging_intent_path(staging), staging_intent_path(quarantine))
                _append_staging_journal(destination, {
                    "event": "quarantined-redundant-authenticated-staging",
                    "staging": str(staging), "quarantine": str(quarantine),
                })
                return "reused"
            os.rename(staging, destination)
            staging_intent_path(staging).unlink(missing_ok=True)
            verify_candidate(destination)
            _append_staging_journal(destination, {
                "event": "published-authenticated-staging", "staging": str(staging),
            })
            return "installed"
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def install_immutable_tree(
    root: Path,
    destination: Path,
    populate: Callable[[Path], None],
    verify_candidate: Callable[[Path], None],
    *,
    after_verify: Callable[[Path], None] | None = None,
) -> str:
    """Install a verified tree once; canonical paths are never replaced.

    A lock serializes cooperating installers.  Each contender verifies its own
    staging tree before publication; after taking the publish lock it either
    reuses and verifies the winner or renames into an observed-absent path.
    No path moves a canonical tree out of place, so SIGKILL cannot create the
    old replacement hole.
    """
    durable = resolve_durable_root(root)
    destination = require_descendant(destination, durable)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with _staging_lifecycle_lock(durable, destination):
        recovered = _recover_abandoned_staging(durable, destination, verify_candidate)
        if recovered is not None:
            return _publish_or_quarantine_recovered_staging(
                durable, destination, recovered, verify_candidate
            )
        if destination.exists():
            if destination.is_symlink() or not destination.is_dir():
                raise Refused(f"canonical destination is not a safe tree: {destination}")
            verify_candidate(destination)
            return "reused"

        staging = Path(
            tempfile.mkdtemp(
                dir=destination.parent,
                prefix=f".{destination.name}.staging-{os.getpid()}-",
            )
        )
        # Publish directory ownership while the lifecycle lock still excludes
        # recovery.  A reader sees either neither object or both, never a
        # cooperating installer's bare staging directory.
        intent = write_staging_intent(staging, destination, os.getpid(), "materializing")
    try:
        populate(staging)
        verify_candidate(staging)
        write_staging_intent(
            staging, destination, os.getpid(), "verified", sha256_tree(staging)
        )
        if after_verify is not None:
            after_verify(staging)
        with _staging_lifecycle_lock(durable, destination):
            lock_path = destination.parent / f".{destination.name}.install.lock"
            require_descendant(lock_path, durable)
            with lock_path.open("a+", encoding="utf-8") as lock:
                fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
                try:
                    if destination.exists():
                        if destination.is_symlink() or not destination.is_dir():
                            raise Refused(
                                f"canonical destination is not a safe tree: {destination}"
                            )
                        verify_candidate(destination)
                        quarantine = (
                            durable
                            / "evidence/bootstrap-staging-quarantine"
                            / destination.name
                            / staging.name
                        )
                        quarantine.parent.mkdir(parents=True, exist_ok=True)
                        if quarantine.exists():
                            raise Refused(f"staging quarantine collision: {quarantine}")
                        os.rename(staging, quarantine)
                        os.rename(staging_intent_path(staging), staging_intent_path(quarantine))
                        _append_staging_journal(destination, {
                            "event": "quarantined-redundant-authenticated-staging",
                            "staging": str(staging), "quarantine": str(quarantine),
                        })
                        return "reused"
                    os.rename(staging, destination)
                finally:
                    fcntl.flock(lock.fileno(), fcntl.LOCK_UN)
        verify_candidate(destination)
        intent.unlink(missing_ok=True)
        return "installed"
    finally:
        if staging.exists():
            with _staging_lifecycle_lock(durable, destination):
                try:
                    state = _read_staging_intent(staging, destination, durable)
                    verified = (
                        state.get("lifecycle") == "verified"
                        and state.get("candidate_sha256") == sha256_tree(staging)
                    )
                except Refused:
                    verified = True
                if verified:
                    _append_staging_journal(destination, {
                        "event": "preserved-verified-staging-after-caught-error",
                        "staging": str(staging),
                    })
                else:
                    try:
                        verify_candidate(staging)
                    except Refused:
                        _append_staging_journal(destination, {
                            "event": "removed-unverified-staging-after-caught-error",
                            "staging": str(staging),
                        })
                        shutil.rmtree(staging)
                        intent.unlink(missing_ok=True)
                    else:
                        _append_staging_journal(destination, {
                            "event": "preserved-ambiguous-staging-after-caught-error",
                            "staging": str(staging),
                        })


def _safe_extract(archive: Path, destination: Path) -> Path:
    with tarfile.open(archive) as source:
        _extract_safe_members(source, destination)
    roots = [item for item in destination.iterdir() if item.is_dir()]
    return roots[0] if len(roots) == 1 else destination


def _extract_safe_members(source: tarfile.TarFile, destination: Path) -> None:
    """Extract only regular files/directories with traversal-safe names."""
    members = source.getmembers()
    for member in members:
        if member.issym() or member.islnk() or member.isdev():
            raise Refused(f"archive contains unsupported link or device: {member.name}")
        resolved = (destination / member.name).resolve(strict=False)
        try:
            resolved.relative_to(destination.resolve())
        except ValueError as exc:
            raise Refused(f"archive member escapes destination: {member.name}") from exc
    safe_members = [member for member in members if member.isfile() or member.isdir()]
    try:
        source.extractall(destination, filter="data")
    except TypeError:
        # Python 3.11 lacks tarfile's ``filter`` API. The explicit fences above
        # retain the data-only behavior before invoking its compatible API.
        source.extractall(destination, members=safe_members)


def destdir_payload(destination: Path, install_root: Path) -> Path:
    """Return the DESTDIR payload for an absolute final installation prefix."""
    if not destination.is_absolute() or not install_root.is_absolute():
        raise Refused("DESTDIR and installation prefix must be absolute")
    return install_root / destination.relative_to(destination.anchor)


def _merge_payload(payload: Path, staging: Path, install_root: Path) -> None:
    if not payload.is_dir():
        raise Refused(f"installation did not produce DESTDIR payload: {payload}")
    for item in payload.iterdir():
        target = staging / item.name
        if item.is_dir():
            shutil.copytree(item, target, dirs_exist_ok=True, symlinks=True)
        else:
            shutil.copy2(item, target, follow_symlinks=False)
    shutil.rmtree(install_root)


def run(
    argv: list[str], *, cwd: Path | None = None, env: Mapping[str, str] | None = None
) -> str:
    value = subprocess.run(
        argv, cwd=cwd, env=env, capture_output=True, text=True, check=False
    )
    if value.returncode:
        detail = value.stderr.strip() or value.stdout.strip() or "command failed"
        raise Refused(f"{Path(argv[0]).name}: {detail}")
    return value.stdout.strip()


def staged_perl_environment(candidate: Path) -> dict[str, str]:
    """Make a staged Perl see its own libraries before canonical publication."""
    library = candidate / "lib"
    paths = [library, library / "site_perl"]
    paths.extend(sorted((path for path in library.rglob("*") if path.is_dir()), key=str))
    inherited = os.environ.get("PERL5LIB")
    if inherited:
        paths.extend(Path(value) for value in inherited.split(":") if value)
    return {**os.environ, "PERL5LIB": ":".join(dict.fromkeys(map(str, paths)))}


def _verify_perl_tree(root: Path, candidate: Path) -> None:
    perl = candidate / "bin/perl5.38.2"
    if not perl.is_file() or perl.is_symlink():
        raise Refused(f"Perl tree is missing its executable: {candidate}")
    environment = staged_perl_environment(candidate)
    configured_prefix = run([str(perl), "-MConfig", "-e", "print $Config{prefix}"], env=environment)
    validate_perl_prefix(perl_prefix(root), configured_prefix)
    version = run([str(perl), "-MArchive::Zip", "-e", "print $Archive::Zip::VERSION"], env=environment)
    if version != "1.68":
        raise Refused(f"Archive::Zip must be 1.68, got {version}")


def _materialize_perl(root: Path, perl_archive: Path, zip_archive: Path) -> None:
    """Build Perl and Archive::Zip in one immutable staging tree."""
    prefix = perl_prefix(root)

    def populate(staging: Path) -> None:
        install_root = staging / ".destdir"
        source_parent = root / "cache/sources"
        source_parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=source_parent, prefix="perl-build-") as work:
            source = _safe_extract(perl_archive, Path(work))
            run(["sh", "Configure", "-des", f"-Dprefix={prefix}"], cwd=source)
            run(["make", f"-j{min(os.cpu_count() or 1, 4)}"], cwd=source)
            run(["make", "install", f"DESTDIR={install_root}"], cwd=source)
        _merge_payload(destdir_payload(prefix, install_root), staging, install_root)
        perl = staging / "bin/perl5.38.2"
        if not perl.is_file():
            raise Refused("Perl build did not produce perl5.38.2")

        install_root = staging / ".archive-zip-destdir"
        with tempfile.TemporaryDirectory(
            dir=source_parent, prefix="archive-zip-build-"
        ) as work:
            source = _safe_extract(zip_archive, Path(work))
            environment = staged_perl_environment(staging)
            value = subprocess.run(
                [str(perl), "Makefile.PL", f"PREFIX={prefix}"],
                cwd=source,
                env=environment,
                capture_output=True,
                text=True,
                check=False,
            )
            if value.returncode:
                detail = value.stderr.strip() or value.stdout.strip() or "command failed"
                raise Refused(f"perl5.38.2: {detail}")
            run(["make", f"-j{min(os.cpu_count() or 1, 4)}"], cwd=source, env=environment)
            run(["make", "install", f"DESTDIR={install_root}"], cwd=source, env=environment)
        _merge_payload(destdir_payload(prefix, install_root), staging, install_root)

    install_immutable_tree(root, prefix, populate, lambda candidate: _verify_perl_tree(root, candidate))


def _materialize_exiftool(
    root: Path,
    item: Mapping[str, Any],
) -> None:
    destination = exiftool_root(root)

    def populate(staging: Path) -> None:
        run(["git", "clone", "--no-checkout", item["repo"], str(staging)])
        run(["git", "checkout", "--detach", item["tag_object"]], cwd=staging)
        actual = run(["git", "rev-parse", "HEAD"], cwd=staging)
        if actual != item["tag_object"]:
            raise Refused(f"wrong ExifTool tag object: {actual}")

    def verify_candidate(candidate: Path) -> None:
        if not (candidate / "exiftool").is_file() or not (candidate / "lib").is_dir():
            raise Refused(f"ExifTool tree is incomplete: {candidate}")
        actual = run(["git", "rev-parse", "HEAD"], cwd=candidate)
        if actual != item["tag_object"]:
            raise Refused(f"wrong ExifTool tag object: {actual}")

    install_immutable_tree(root, destination, populate, verify_candidate)


def _write_corpus_manifest(root: Path) -> Path:
    corpus = corpus_path(root)
    if not corpus.is_dir():
        raise Refused(f"combined corpus missing: {corpus}")
    manifest = corpus.parent / "combined-samples.manifest"
    lines = []
    for item in sorted(
        (value for value in corpus.rglob("*") if value.is_file()),
        key=lambda value: value.relative_to(corpus).as_posix(),
    ):
        lines.append(f"{sha256_file(item)}  {item.relative_to(corpus).as_posix()}\n")
    if len(lines) < MIN_CORPUS_FILES:
        raise Refused(
            f"corpus needs at least {MIN_CORPUS_FILES} files, got {len(lines)}"
        )
    descriptor, temporary_name = tempfile.mkstemp(
        dir=manifest.parent,
        prefix=f".{manifest.name}.",
        suffix=".partial",
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as output:
            descriptor = -1
            output.write("".join(lines))
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, manifest)
        _fsync_directory(manifest.parent)
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        temporary.unlink(missing_ok=True)
    return manifest


def _materialize_corpus(
    root: Path,
    archives: Mapping[str, Path],
) -> None:
    destination = corpus_path(root)

    def populate(staging: Path) -> None:
        base = exiftool_root(root) / "t/images"
        if not base.is_dir():
            raise Refused(f"ExifTool t/images missing: {base}")
        shutil.copytree(base, staging, dirs_exist_ok=True, symlinks=True)
        for name, archive in sorted(archives.items()):
            if not name.startswith("samples_"):
                continue
            with tarfile.open(archive) as source:
                _extract_safe_members(source, staging)
        count = sum(item.is_file() for item in staging.rglob("*"))
        if count < MIN_CORPUS_FILES:
            raise Refused(
                f"corpus needs at least {MIN_CORPUS_FILES} files, got {count}"
            )

    install_immutable_tree(root, destination, populate, _verify_corpus_tree)


def _verify_corpus_tree(candidate: Path) -> None:
    count = sum(item.is_file() for item in candidate.rglob("*"))
    if count < MIN_CORPUS_FILES:
        raise Refused(f"corpus needs at least {MIN_CORPUS_FILES} files, got {count}")
    expected = str(LOCK.get("corpus_tree_sha256", ""))
    actual = sha256_tree(candidate)
    if actual != expected:
        raise Refused(f"corpus lock hash mismatch: expected {expected}, got {actual}")


def legacy_nested_corpus_path(root: Path = DURABLE_ROOT) -> Path:
    return exiftool_root(root) / "combined-samples"


def legacy_nested_corpus_manifest_path(root: Path = DURABLE_ROOT) -> Path:
    return exiftool_root(root) / "combined-samples.manifest"


def assert_no_legacy_nested_corpus(root: Path) -> None:
    """Refuse the stale nested layout until an operator explicitly quarantines it."""
    durable = resolve_durable_root(root)
    nested = require_descendant(legacy_nested_corpus_path(durable), durable)
    nested_manifest = require_descendant(
        legacy_nested_corpus_manifest_path(durable), durable
    )
    if (
        nested.exists()
        or nested.is_symlink()
        or nested_manifest.exists()
        or nested_manifest.is_symlink()
    ):
        raise Refused(
            "legacy nested corpus is ambiguous; run quarantine-legacy-corpus explicitly: "
            f"{nested} or {nested_manifest}"
        )


def quarantine_legacy_nested_corpus(root: Path) -> dict[str, str]:
    """Journal and quarantine an identical nested corpus without touching sibling data."""
    durable = resolve_durable_root(root)
    nested = require_descendant(legacy_nested_corpus_path(durable), durable)
    nested_manifest = require_descendant(
        legacy_nested_corpus_manifest_path(durable), durable
    )
    sibling = require_descendant(corpus_path(durable), durable)
    journal = evidence_path(durable) / "legacy-nested-corpus-quarantine.json"
    if journal.is_file():
        state = json.loads(journal.read_text(encoding="utf-8"))
        if state.get("status") == "quarantined" and not nested.exists() and not nested_manifest.exists():
            return {key: str(value) for key, value in state.items()}
        if state.get("status") == "prepared":
            recorded = require_descendant(Path(str(state.get("quarantine", ""))), durable)
            recorded_manifest = require_descendant(
                Path(str(state.get("quarantined_manifest", ""))), durable
            )
            expected = str(state.get("sha256", ""))
            if (
                not nested.exists()
                and recorded.is_dir()
                and not recorded.is_symlink()
                and sha256_tree(recorded) == expected
                and sibling.is_dir()
                and sha256_tree(sibling) == expected
            ):
                if nested_manifest.exists():
                    if recorded_manifest.exists() or nested_manifest.is_symlink():
                        raise Refused("legacy nested manifest recovery is ambiguous")
                    os.rename(nested_manifest, recorded_manifest)
                manifest = verify(durable, VERSION, None)
                disposition = {
                    **{key: str(value) for key, value in state.items()},
                    "status": "quarantined",
                    "manifest": str(manifest),
                    "manifest_sha256": sha256_file(manifest),
                }
                atomic_json(journal, disposition)
                return disposition
    if not nested.exists() and not nested_manifest.exists():
        raise Refused(f"legacy nested corpus is absent: {nested}")
    if nested.is_symlink() or not nested.is_dir() or sibling.is_symlink() or not sibling.is_dir():
        raise Refused("ambiguous legacy nested corpus is not two safe directories")
    if nested_manifest.exists() and (nested_manifest.is_symlink() or not nested_manifest.is_file()):
        raise Refused("ambiguous legacy nested corpus manifest is unsafe")
    sibling_hash = sha256_tree(sibling)
    nested_hash = sha256_tree(nested)
    if sibling_hash != nested_hash:
        raise Refused(
            "ambiguous legacy nested corpus differs from sibling corpus; refusing mutation"
        )
    if nested_hash != LOCK["corpus_tree_sha256"]:
        raise Refused("legacy nested corpus does not match the locked sibling corpus")
    quarantine = (
        evidence_path(durable)
        / "legacy-nested-corpus-quarantine"
        / f"{nested_hash}-{VERSION}"
    )
    quarantined_manifest = quarantine.parent / f"{quarantine.name}.manifest"
    require_descendant(quarantine, durable)
    require_descendant(quarantined_manifest, durable)
    if quarantine.exists():
        raise Refused(f"legacy corpus quarantine destination already exists: {quarantine}")
    atomic_json(
        journal,
        {
            "status": "prepared",
            "nested": str(nested),
            "sibling": str(sibling),
            "sha256": nested_hash,
            "quarantine": str(quarantine),
            "nested_manifest": str(nested_manifest),
            "quarantined_manifest": str(quarantined_manifest),
        },
    )
    quarantine.parent.mkdir(parents=True, exist_ok=True)
    os.rename(nested, quarantine)
    if nested_manifest.exists():
        os.rename(nested_manifest, quarantined_manifest)
    manifest = verify(durable, VERSION, None)
    disposition = {
        "status": "quarantined",
        "nested": str(nested),
        "sibling": str(sibling),
        "sha256": nested_hash,
        "quarantine": str(quarantine),
        "nested_manifest": str(nested_manifest),
        "quarantined_manifest": str(quarantined_manifest),
        "manifest": str(manifest),
        "manifest_sha256": sha256_file(manifest),
    }
    atomic_json(journal, disposition)
    return disposition


def authenticate_artifacts(
    root: Path, artifacts: Mapping[str, Path]
) -> dict[str, dict[str, Any]]:
    authenticated: dict[str, dict[str, Any]] = {}
    for name, raw_path in artifacts.items():
        path = require_descendant(raw_path, root)
        if not path.exists():
            raise Refused(f"artifact missing: {path}")
        authenticated[name] = {
            "path": str(path),
            "kind": "tree" if path.is_dir() else "file",
            "sha256": sha256_tree(path) if path.is_dir() else sha256_file(path),
        }
    return authenticated


def validate_versions(
    perl: str, archive: str, exiftool: str, docx: str, files: int
) -> None:
    if perl != "v5.38.2":
        raise Refused(f"Perl must be v5.38.2, got {perl}")
    if archive != "1.68":
        raise Refused(f"Archive::Zip must be 1.68, got {archive}")
    if exiftool != VERSION:
        raise Refused(f"ExifTool must be {VERSION}, got {exiftool}")
    if docx != "DOCX":
        raise Refused(f"DOCX probe must report DOCX, got {docx}")
    if files < MIN_CORPUS_FILES:
        raise Refused(
            f"corpus needs at least {MIN_CORPUS_FILES} files, got {files}"
        )


def validate_perl_prefix(expected: Path, actual: str) -> None:
    expected_value = Path(os.path.abspath(expected.expanduser()))
    actual_value = Path(os.path.abspath(Path(actual).expanduser()))
    if actual_value != expected_value:
        raise Refused(
            f"Perl configured prefix must be {expected_value}, got {actual_value}"
        )


def validate_manifest(manifest: Mapping[str, Any], root: Path) -> None:
    durable = resolve_durable_root(root)
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, Mapping) or not artifacts:
        raise Refused("manifest missing hash artifacts")
    for name, raw_item in artifacts.items():
        if not isinstance(raw_item, Mapping) or len(str(raw_item.get("sha256", ""))) != 64:
            raise Refused(f"manifest artifact {name} missing hash")
        path_value = raw_item.get("path")
        if path_value:
            require_descendant(Path(str(path_value)), durable)


def required_artifact_names() -> set[str]:
    return {
        *(f"{name}_source" for name in LOCK["archives"]),
        "perl_tree",
        "perl_executable",
        "archive_zip_library",
        "exiftool_tree",
        "exiftool_executable",
        "exiftool_library",
        "corpus_tree",
        "corpus_manifest",
    }


def _artifact_hash(path: Path, kind: str) -> str:
    if kind == "tree":
        if not path.is_dir():
            raise Refused(f"manifest tree artifact is missing: {path}")
        return sha256_tree(path)
    if kind == "file":
        if not path.is_file():
            raise Refused(f"manifest file artifact is missing: {path}")
        return sha256_file(path)
    raise Refused(f"manifest artifact has invalid kind {kind!r}: {path}")


def validate_locked_manifest(
    manifest: Mapping[str, Any],
    root: Path,
    *,
    required: set[str] | None = None,
) -> None:
    """Authenticate locked identities plus every recorded tree/library byte."""
    durable = resolve_durable_root(root)
    validate_manifest(manifest, durable)
    if manifest.get("schema_version") != 1:
        raise Refused("manifest schema_version must be 1")
    if manifest.get("lock_sha256") != sha256_file(LOCK_PATH):
        raise Refused("manifest lock hash mismatch")
    expected_identities = {
        "perl": LOCK["perl"],
        "archive_zip": LOCK["archive_zip"],
        "exiftool": LOCK["exiftool"],
        "corpus_tree_sha256": LOCK["corpus_tree_sha256"],
    }
    if manifest.get("source_identities") != expected_identities:
        raise Refused("manifest source identities do not match the oracle lock")
    artifacts = manifest["artifacts"]
    needed = required_artifact_names() if required is None else required
    missing = sorted(needed - set(artifacts))
    if missing:
        raise Refused(f"manifest missing required artifact {missing[0]}")
    for name in sorted(needed):
        item = artifacts[name]
        path = require_descendant(Path(str(item.get("path", ""))), durable)
        actual = _artifact_hash(path, str(item.get("kind", "")))
        if actual != item["sha256"]:
            raise Refused(
                f"manifest artifact {name} hash mismatch: "
                f"expected {item['sha256']}, got {actual}"
            )


def manifest_repair_components(
    manifest: Mapping[str, Any], root: Path
) -> set[str]:
    """Map authenticated artifact damage to the smallest rebuildable component."""
    durable = resolve_durable_root(root)
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, Mapping):
        return set()
    component_by_artifact = {
        "perl_tree": "perl",
        "perl_executable": "perl",
        "archive_zip_library": "perl",
        "exiftool_tree": "exiftool",
        "exiftool_executable": "exiftool",
        "exiftool_library": "exiftool",
        "corpus_tree": "corpus",
        "corpus_manifest": "corpus",
    }
    repairs: set[str] = set()
    for name, component in component_by_artifact.items():
        item = artifacts.get(name)
        # A manifest from before an artifact was enumerated is migrated by the
        # final probes; absence alone is not evidence that installed bytes are
        # damaged.
        if not isinstance(item, Mapping):
            continue
        try:
            path = require_descendant(Path(str(item.get("path", ""))), durable)
            actual = _artifact_hash(path, str(item.get("kind", "")))
        except Refused:
            repairs.add(component)
            continue
        if actual != item.get("sha256"):
            repairs.add(component)
    if "exiftool" in repairs:
        repairs.add("corpus")
    return repairs


def _verify_archive_sources(root: Path) -> dict[str, Path]:
    sources: dict[str, Path] = {}
    for name, raw_item in LOCK["archives"].items():
        item = dict(raw_item)
        source = root / "cache/downloads" / item["filename"]
        if not source.is_file():
            raise Refused(f"locked archive missing: {source}")
        actual = sha256_file(source)
        if actual != item["sha256"]:
            raise Refused(f"locked archive hash mismatch: {source}")
        sources[name] = source
    return sources


def materialize(
    root: Path,
    archives: Mapping[str, Mapping[str, str]],
) -> dict[str, dict[str, Any]]:
    """Materialize immutable locked artifacts or reuse exact canonical trees."""
    validate_lock(LOCK)
    downloaded = {
        name: download_locked(root, name, item) for name, item in archives.items()
    }
    perl_archive = downloaded["perl"]
    zip_archive = downloaded["archive_zip"]
    _materialize_perl(root, perl_archive, zip_archive)
    _materialize_exiftool(root, LOCK["exiftool"])
    _materialize_corpus(root, downloaded)
    corpus_manifest = _write_corpus_manifest(root)

    named: dict[str, Path] = {
        f"{name}_source": path for name, path in downloaded.items()
    }
    named.update(
        {
            "perl_tree": perl_prefix(root),
            "perl_executable": perl_path(root),
            "archive_zip_library": archive_zip_library_path(root),
            "exiftool_tree": exiftool_root(root),
            "exiftool_executable": exiftool_path(root),
            "exiftool_library": exiftool_library_path(root),
            "corpus_tree": corpus_path(root),
            "corpus_manifest": corpus_manifest,
        }
    )
    return authenticate_artifacts(root, named)


def verify(root: Path, pin: str, manifest: Path | None) -> Path:
    durable = resolve_durable_root(root)
    validate_lock(LOCK)
    assert_no_legacy_nested_corpus(durable)
    if pin != VERSION:
        raise Refused(f"ExifTool pin must be {VERSION}, got {pin}")
    perl = require_descendant(perl_path(durable), durable)
    exiftool = require_descendant(exiftool_path(durable), durable)
    corpus = require_descendant(corpus_path(durable), durable)
    docx = require_descendant(exiftool.parent / "t/images/OOXML.docx", durable)
    for path in (perl, exiftool, corpus, docx):
        if not path.exists():
            raise Refused(f"durable artifact is missing: {path}")
    _verify_corpus_tree(corpus)
    sources = _verify_archive_sources(durable)
    tag_object = run(["git", "rev-parse", "HEAD"], cwd=exiftool.parent)
    if tag_object != LOCK["exiftool"]["tag_object"]:
        raise Refused(f"wrong ExifTool tag object: {tag_object}")
    values = (
        run([str(perl), "-e", "print $^V"]),
        run([str(perl), "-MArchive::Zip", "-e", "print $Archive::Zip::VERSION"]),
        run([str(perl), "-I", str(exiftool.parent / "lib"), str(exiftool), "-ver"]),
        run(
            [
                str(perl),
                "-I",
                str(exiftool.parent / "lib"),
                str(exiftool),
                "-config",
                "",
                "-s",
                "-s",
                "-s",
                "-FileType",
                str(docx),
            ]
        ),
        sum(item.is_file() for item in corpus.rglob("*")),
    )
    validate_versions(*values)
    configured_prefix = run(
        [str(perl), "-MConfig", "-e", "print $Config{prefix}"]
    )
    validate_perl_prefix(perl_prefix(durable), configured_prefix)
    corpus_manifest = _write_corpus_manifest(durable)
    named: dict[str, Path] = {
        f"{name}_source": path for name, path in sources.items()
    }
    named.update(
        {
            "perl_tree": perl_prefix(durable),
            "perl_executable": perl,
            "archive_zip_library": archive_zip_library_path(durable),
            "exiftool_tree": exiftool.parent,
            "exiftool_executable": exiftool,
            "exiftool_library": exiftool_library_path(durable),
            "corpus_tree": corpus,
            "corpus_manifest": corpus_manifest,
        }
    )
    payload: dict[str, Any] = {
        "schema_version": 1,
        "lock_sha256": sha256_file(LOCK_PATH),
        "artifacts": authenticate_artifacts(durable, named),
        "source_identities": {
            "perl": LOCK["perl"],
            "archive_zip": LOCK["archive_zip"],
            "exiftool": LOCK["exiftool"],
            "corpus_tree_sha256": LOCK["corpus_tree_sha256"],
        },
        "probes": {
            "perl": values[0],
            "perl_prefix": configured_prefix,
            "archive_zip": values[1],
            "exiftool": values[2],
            "docx": values[3],
            "corpus_files": values[4],
        },
    }
    output = manifest_path(durable)
    if manifest is not None:
        requested = require_descendant(manifest, durable)
        if requested != output:
            raise Refused(f"manifest path must be {output}, not {requested}")
        if requested.exists():
            validate_locked_manifest(
                json.loads(requested.read_text(encoding="utf-8")), durable
            )
    atomic_json(output, payload)
    return output


def provision(root: Path) -> Path:
    durable = resolve_durable_root(root)
    journal = evidence_path(durable) / "bootstrap-journal.json"
    stage = "validate-lock"
    try:
        validate_lock(LOCK)
        assert_no_legacy_nested_corpus(durable)
        existing_manifest = manifest_path(durable)
        if existing_manifest.is_file():
            try:
                existing = json.loads(existing_manifest.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                raise Refused("existing durable manifest is unreadable; explicit repair is required")
            validate_locked_manifest(existing, durable)
        stage = "materialize"
        artifacts = materialize(durable, LOCK["archives"])
        stage = "verify"
        output = verify(durable, VERSION, None)
        payload = json.loads(output.read_text(encoding="utf-8"))
        payload["artifacts"].update(artifacts)
        atomic_json(output, payload)
        atomic_json(
            journal,
            {"stage": "complete", "manifest": str(output), "manifest_sha256": sha256_file(output)},
        )
        return output
    except Exception as exc:
        atomic_json(journal, {"failed_stage": stage, "error": str(exc)})
        if isinstance(exc, Refused):
            raise
        raise Refused(f"{stage} failed: {exc}") from exc


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    subcommands = parser.add_subparsers(dest="command", required=True)
    for name in (
        "provision",
        "verify",
        "check-path",
        "quarantine-legacy-corpus",
        "write-comparison-wrapper",
        "cleanup-comparison-workdir",
    ):
        command = subcommands.add_parser(name)
        command.add_argument("--root", type=Path, required=True)
        if name == "verify":
            command.add_argument("--pin", required=True)
            command.add_argument("--manifest", type=Path)
        if name == "check-path":
            command.add_argument("--path", type=Path, required=True)
        if name == "write-comparison-wrapper":
            command.add_argument("--target", type=Path, required=True)
            command.add_argument("--perl", type=Path, required=True)
            command.add_argument("--library", type=Path, required=True)
            command.add_argument("--script", type=Path, required=True)
            command.add_argument("--marker-directory", type=Path, required=True)
        if name == "cleanup-comparison-workdir":
            command.add_argument("--work-directory", type=Path, required=True)
            command.add_argument("--receipt-root", type=Path, required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        if args.command == "provision":
            output = provision(args.root)
        elif args.command == "verify":
            output = verify(args.root, args.pin, args.manifest)
        elif args.command == "quarantine-legacy-corpus":
            output = quarantine_legacy_nested_corpus(args.root)["manifest"]
        elif args.command == "write-comparison-wrapper":
            root = resolve_durable_root(args.root)
            write_comparison_wrapper(
                require_descendant(args.target, root),
                require_descendant(args.perl, root),
                require_descendant(args.library, root),
                require_descendant(args.script, root),
                require_descendant(args.marker_directory, root),
            )
            output = args.target
        elif args.command == "cleanup-comparison-workdir":
            root = resolve_durable_root(args.root)
            cleanup_comparison_workdir(
                require_descendant(args.work_directory, root),
                require_descendant(args.receipt_root, root),
            )
            output = args.receipt_root
        else:
            root = resolve_durable_root(args.root)
            output = resolve_durable_override(args.path, root)
        print(output)
        return 0
    except Refused as exc:
        print(f"REFUSED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
