#!/usr/bin/env python3
"""Create and verify release assets against an independent run manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys


PAYLOAD_NAMES = (
    "oxidex-x86_64-unknown-linux-musl",
    "oxidex-aarch64-unknown-linux-musl",
    "oxidex-x86_64-pc-windows-gnu.exe",
    "oxidex-universal-apple-darwin",
)


class ReleaseAssetError(ValueError):
    """The release asset set or its provenance is invalid."""


def expected_payload_names(version: str) -> tuple[str, ...]:
    return (*PAYLOAD_NAMES, f"oxidex-v{version}.dmg")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_exact_files(directory: pathlib.Path, expected: set[str]) -> None:
    actual = {path.name for path in directory.iterdir() if path.is_file()}
    if actual != expected:
        raise ReleaseAssetError(
            f"asset set mismatch: expected {sorted(expected)!r}, got {sorted(actual)!r}")
    for name in expected:
        path = directory / name
        if not path.is_file() or path.stat().st_size <= 0:
            raise ReleaseAssetError(f"asset is missing, not regular, or empty: {name}")


def checksum_text(assets: pathlib.Path, names: tuple[str, ...]) -> str:
    return "".join(f"{sha256(assets / name)}  {name}\n" for name in sorted(names))


def identity_from_args(args: argparse.Namespace) -> dict[str, str | int]:
    return {
        "schema_version": 1,
        "version": args.version,
        "tag": args.tag,
        "head_sha": args.head_sha,
        "run_id": args.run_id,
        "run_attempt": args.run_attempt,
    }


def create_provenance(args: argparse.Namespace) -> None:
    assets = pathlib.Path(args.assets)
    provenance = pathlib.Path(args.provenance)
    payload_names = expected_payload_names(args.version)
    require_exact_files(assets, set(payload_names))
    provenance.mkdir()
    checksums = checksum_text(assets, payload_names)
    (assets / "SHA256SUMS").write_text(checksums)
    (provenance / "SHA256SUMS").write_text(checksums)
    manifest = identity_from_args(args)
    manifest["payloads"] = list(payload_names)
    (provenance / "provenance.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def verify_assets(args: argparse.Namespace) -> None:
    assets = pathlib.Path(args.assets)
    provenance = pathlib.Path(args.provenance)
    payload_names = expected_payload_names(args.version)
    expected_names = {*payload_names, "SHA256SUMS"}
    require_exact_files(assets, expected_names)
    require_exact_files(provenance, {"SHA256SUMS", "provenance.json"})
    manifest = json.loads((provenance / "provenance.json").read_text())
    expected_manifest = identity_from_args(args)
    expected_manifest["payloads"] = list(payload_names)
    if manifest != expected_manifest:
        raise ReleaseAssetError("provenance identity does not match this workflow run")
    expected_checksums = (provenance / "SHA256SUMS").read_text()
    if (assets / "SHA256SUMS").read_text() != expected_checksums:
        raise ReleaseAssetError("released SHA256SUMS differs from run provenance")
    if checksum_text(assets, payload_names) != expected_checksums:
        raise ReleaseAssetError("released asset digest differs from run provenance")
    print(f"verified {len(expected_names)} exact release assets")


def parse_bool(value: str) -> bool:
    if value == "true":
        return True
    if value == "false":
        return False
    raise argparse.ArgumentTypeError("expected true or false")


def verify_release(args: argparse.Namespace) -> None:
    release = json.loads(pathlib.Path(args.release_json).read_text())
    expected = {
        "id": int(args.release_id),
        "tag_name": args.tag,
        "draft": args.draft,
        "prerelease": args.prerelease,
    }
    for field, value in expected.items():
        if release.get(field) != value:
            raise ReleaseAssetError(
                f"release {field} mismatch: expected {value!r}, got {release.get(field)!r}")
    if args.resolved_target != args.head_sha:
        raise ReleaseAssetError(
            f"remote tag target mismatch: expected {args.head_sha!r}, "
            f"got {args.resolved_target!r}")
    expected_names = {*expected_payload_names(args.version), "SHA256SUMS"}
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise ReleaseAssetError("release assets must be a list")
    actual_names = [asset.get("name") for asset in assets]
    if len(actual_names) != len(set(actual_names)) or set(actual_names) != expected_names:
        raise ReleaseAssetError(
            f"release asset set mismatch: expected {sorted(expected_names)!r}, "
            f"got {sorted(str(name) for name in actual_names)!r}")
    if any(not isinstance(asset.get("size"), int) or asset["size"] <= 0 for asset in assets):
        raise ReleaseAssetError("release assets must all have a positive byte size")
    state = "draft" if args.draft else "published"
    print(f"verified {state} release {args.release_id}")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    subcommands = result.add_subparsers(dest="command", required=True)
    for name in ("create-provenance", "verify-assets"):
        command = subcommands.add_parser(name)
        command.add_argument("--assets", required=True)
        command.add_argument("--provenance", required=True)
        command.add_argument("--version", required=True)
        command.add_argument("--tag", required=True)
        command.add_argument("--head-sha", required=True)
        command.add_argument("--run-id", required=True)
        command.add_argument("--run-attempt", required=True)
    release = subcommands.add_parser("verify-release")
    release.add_argument("--release-json", required=True)
    release.add_argument("--release-id", required=True)
    release.add_argument("--version", required=True)
    release.add_argument("--tag", required=True)
    release.add_argument("--head-sha", required=True)
    release.add_argument("--run-id", required=True)
    release.add_argument("--run-attempt", required=True)
    release.add_argument("--draft", required=True, type=parse_bool)
    release.add_argument("--prerelease", required=True, type=parse_bool)
    release.add_argument("--resolved-target", required=True)
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "create-provenance":
            create_provenance(args)
        elif args.command == "verify-assets":
            verify_assets(args)
        else:
            verify_release(args)
    except (OSError, ReleaseAssetError, json.JSONDecodeError) as error:
        print(f"release assets refused: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
