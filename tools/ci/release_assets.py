#!/usr/bin/env python3
"""Create and verify release assets against an independent run manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
import tomllib


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


def sbom_name(version: str) -> str:
    return f"oxidex-v{version}.sbom.cdx.json"


def expected_asset_names(version: str) -> tuple[str, ...]:
    return (*expected_payload_names(version), sbom_name(version), "SHA256SUMS")


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


def cargo_components(source_root: pathlib.Path) -> tuple[dict[str, object], str]:
    lock_path = source_root / "Cargo.lock"
    cargo_path = source_root / "Cargo.toml"
    if not lock_path.is_file() or not cargo_path.is_file():
        raise ReleaseAssetError("source root must contain Cargo.toml and Cargo.lock")
    try:
        packages = tomllib.loads(lock_path.read_text()).get("package", [])
    except tomllib.TOMLDecodeError as error:
        raise ReleaseAssetError(f"Cargo.lock is not valid TOML: {error}") from error
    if not isinstance(packages, list):
        raise ReleaseAssetError("Cargo.lock package list is invalid")
    components = []
    for package in packages:
        if not isinstance(package, dict) or not isinstance(package.get("name"), str) \
                or not isinstance(package.get("version"), str):
            raise ReleaseAssetError("Cargo.lock package entry is invalid")
        name = package["name"]
        version = package["version"]
        components.append({
            "type": "library", "name": name, "version": version,
            "purl": f"pkg:cargo/{name}@{version}",
        })
    return tuple(components), sha256(lock_path)


def sbom_document(args: argparse.Namespace, assets: pathlib.Path) -> dict[str, object]:
    source_root = pathlib.Path(args.source_root)
    packages, lock_digest = cargo_components(source_root)
    payloads = expected_payload_names(args.version)
    files = tuple({
        "type": "file", "name": name,
        "hashes": [{"alg": "SHA-256", "content": sha256(assets / name)}],
    } for name in payloads)
    components = sorted((*packages, *files), key=lambda item: (
        str(item["type"]), str(item["name"]), str(item.get("version", ""))))
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "component": {"type": "application", "name": "oxidex", "version": args.version},
            "properties": [{
                "name": "org.oxidex.source.cargo_lock.sha256", "value": lock_digest,
            }],
        },
        "components": components,
    }


def create_provenance(args: argparse.Namespace) -> None:
    assets = pathlib.Path(args.assets)
    provenance = pathlib.Path(args.provenance)
    payload_names = expected_payload_names(args.version)
    require_exact_files(assets, set(payload_names))
    provenance.mkdir()
    sbom = sbom_document(args, assets)
    sbom_path = assets / sbom_name(args.version)
    sbom_path.write_text(json.dumps(sbom, sort_keys=True, separators=(",", ":")) + "\n")
    checksummed_names = (*payload_names, sbom_path.name)
    checksums = checksum_text(assets, checksummed_names)
    (assets / "SHA256SUMS").write_text(checksums)
    (provenance / "SHA256SUMS").write_text(checksums)
    manifest = identity_from_args(args)
    manifest["payloads"] = list(payload_names)
    manifest["sbom"] = {"name": sbom_path.name, "sha256": sha256(sbom_path)}
    manifest["source"] = {
        "cargo_lock_sha256": sbom["metadata"]["properties"][0]["value"],
    }
    (provenance / "provenance.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def verify_assets(args: argparse.Namespace) -> None:
    assets = pathlib.Path(args.assets)
    provenance = pathlib.Path(args.provenance)
    payload_names = expected_payload_names(args.version)
    expected_names = set(expected_asset_names(args.version))
    require_exact_files(assets, expected_names)
    require_exact_files(provenance, {"SHA256SUMS", "provenance.json"})
    manifest = json.loads((provenance / "provenance.json").read_text())
    expected_manifest = identity_from_args(args)
    if any(manifest.get(field) != value for field, value in expected_manifest.items()) \
            or manifest.get("payloads") != list(payload_names):
        raise ReleaseAssetError("provenance identity does not match this workflow run")
    sbom = manifest.get("sbom")
    source = manifest.get("source")
    if not isinstance(sbom, dict) or sbom.get("name") != sbom_name(args.version) \
            or not isinstance(sbom.get("sha256"), str) \
            or not isinstance(source, dict) \
            or not isinstance(source.get("cargo_lock_sha256"), str):
        raise ReleaseAssetError("provenance SBOM binding is invalid")
    sbom_path = assets / sbom_name(args.version)
    if sha256(sbom_path) != sbom["sha256"]:
        raise ReleaseAssetError("released SBOM differs from run provenance")
    expected_checksums = (provenance / "SHA256SUMS").read_text()
    if (assets / "SHA256SUMS").read_text() != expected_checksums:
        raise ReleaseAssetError("released SHA256SUMS differs from run provenance")
    if checksum_text(assets, (*payload_names, sbom_path.name)) != expected_checksums:
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
    expected_names = set(expected_asset_names(args.version))
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
    if not args.draft:
        if not args.latest_json:
            raise ReleaseAssetError("published release requires latest API evidence")
        latest = json.loads(pathlib.Path(args.latest_json).read_text())
        if not isinstance(latest, dict):
            raise ReleaseAssetError("latest API response must be an object")
        if args.prerelease:
            if latest.get("tag_name") == args.tag or latest.get("draft") is not False \
                    or latest.get("prerelease") is not False:
                raise ReleaseAssetError("prerelease must not become GitHub Latest")
        elif latest.get("tag_name") != args.tag or latest.get("draft") is not False \
                or latest.get("prerelease") is not False:
            raise ReleaseAssetError("stable release did not become GitHub Latest")
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
        if name == "create-provenance":
            command.add_argument("--source-root", required=True)
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
    release.add_argument("--latest-json")
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
