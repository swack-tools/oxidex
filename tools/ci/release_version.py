#!/usr/bin/env python3
"""Classify a release tag and refuse one that disagrees with Cargo.toml.

Run by the `verify-version` job of .github/workflows/release.yml before any
artifact is built. It prints, and appends to $GITHUB_OUTPUT when set:

    version=<tag without the leading v>      e.g. 2.0.0-beta.1
    prerelease=true|false                    true iff the version has a '-'

A SemVer pre-release is exactly a version with a `-<identifiers>` suffix
(SemVer 2.0.0 section 9), so `2.0.0-beta.1` and `2.0.0-rc.1` are
pre-releases and `2.0.0` is not. Build metadata (`+...`) is not a pre-release
marker and is rejected outright: cargo ignores it for ordering, so a tag
carrying it cannot name a distinct release.

Why this exists: release.yml used to hard-code `prerelease: false`, so a
`v2.0.0-beta.1` tag would have been published as a full GitHub release and
marked Latest. The tag/Cargo.toml equality check stops the other quiet
failure -- a tag pushed on a commit whose crate version was never bumped,
which builds, signs and ships binaries whose `--version` names a different
release than the page they are downloaded from.

Usage: release_version.py [--tag vX.Y.Z[-pre]] [--cargo-toml PATH]
       (the tag defaults to $GITHUB_REF_NAME)
"""
from __future__ import annotations

import argparse
import os
import pathlib
import re
import sys
import tomllib

# SemVer 2.0.0 core plus optional pre-release; no build metadata (see above).
_IDENT = r"(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    rf"(?:-({_IDENT}(?:\.{_IDENT})*))?$"
)


class ReleaseVersionError(ValueError):
    pass


def classify(tag: str, cargo_version: str) -> tuple[str, bool]:
    """Return (version, is_prerelease) for `tag`, or raise ReleaseVersionError."""
    if not tag.startswith("v"):
        raise ReleaseVersionError(f"tag {tag!r} does not start with 'v'")
    version = tag[1:]
    match = SEMVER.match(version)
    if match is None:
        raise ReleaseVersionError(
            f"tag {tag!r} is not v<SemVer> (MAJOR.MINOR.PATCH[-PRERELEASE], no +BUILD)")
    if version != cargo_version:
        raise ReleaseVersionError(
            f"tag {tag!r} names {version} but Cargo.toml [package] version is "
            f"{cargo_version}; bump Cargo.toml (and Cargo.lock) before tagging")
    return version, match.group(4) is not None


def cargo_package_version(path: pathlib.Path) -> str:
    with path.open("rb") as fh:
        return tomllib.load(fh)["package"]["version"]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--tag", default=os.environ.get("GITHUB_REF_NAME", ""))
    parser.add_argument("--cargo-toml", type=pathlib.Path, default=pathlib.Path("Cargo.toml"))
    args = parser.parse_args(argv)
    try:
        version, prerelease = classify(args.tag, cargo_package_version(args.cargo_toml))
    except ReleaseVersionError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        return 1
    lines = [f"version={version}", f"prerelease={'true' if prerelease else 'false'}"]
    print("\n".join(lines))
    out = os.environ.get("GITHUB_OUTPUT")
    if out:
        with open(out, "a", encoding="utf-8") as fh:
            fh.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
