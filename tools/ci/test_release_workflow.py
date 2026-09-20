"""Pre-release handling in the release pipeline: tag classification and the workflow wiring.

A SemVer pre-release tag (v2.0.0-beta.1) must publish as a GitHub
pre-release that never takes the Latest badge, must not relabel the stable
docs site, and must not move Docker's :latest or any other floating tag.
Every one of those used to be wrong -- release.yml hard-coded
`prerelease: false` -- and each fails silently: the release goes out, it is
just labelled as something it is not. These tests read the workflow files as
text (no YAML dependency: this suite runs on a bare `python3`) and pin the
exact expressions, so loosening any of them is a visible diff to this file.
"""
import importlib.util
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import tomllib
import unittest
from unittest import mock

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parent.parent
WORKFLOWS = REPO / ".github" / "workflows"
RELEASE_ASSETS = HERE / "release_assets.py"
VERIFY_MACOS_RELEASE = HERE / "verify_macos_release.sh"
RELEASE_GUIDE = REPO / "docs" / "RELEASE-2.0.0-beta.1.md"

spec = importlib.util.spec_from_file_location("release_version", HERE / "release_version.py")
rv = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = rv
spec.loader.exec_module(rv)


def job_block(workflow_text: str, job: str) -> str:
    """The text of one job under `jobs:` (two-space indent), up to the next job."""
    lines = workflow_text.splitlines()
    start = next((i for i, line in enumerate(lines) if line == f"  {job}:"), None)
    if start is None:
        raise AssertionError(f"job {job!r} not found")
    end = next((i for i in range(start + 1, len(lines))
                if re.match(r"^  [A-Za-z0-9_-]+:\s*$", lines[i])), len(lines))
    return "\n".join(lines[start:end])


def job_needs(block: str) -> set[str]:
    match = re.search(r"^    needs:\s*(.+)$", block, re.M)
    if not match:
        return set()
    value = match.group(1).strip()
    if value.startswith("["):
        return {part.strip() for part in value.strip("[]").split(",") if part.strip()}
    return {value}


def step_run(block: str, name: str) -> str:
    """Extract one literal run block from a workflow job."""
    marker = f"      - name: {name}\n"
    start = block.index(marker) + len(marker)
    end = block.find("\n      - name: ", start)
    step = block[start:end if end != -1 else len(block)]
    run_marker = "        run: |\n"
    run_start = step.index(run_marker) + len(run_marker)
    lines = []
    for line in step[run_start:].splitlines():
        if line and not line.startswith("          "):
            break
        lines.append(line[10:] if line else "")
    return "\n".join(lines) + "\n"


class ClassifyTests(unittest.TestCase):
    def test_beta_is_prerelease(self):
        self.assertEqual(rv.classify("v2.0.0-beta.1", "2.0.0-beta.1"), ("2.0.0-beta.1", True))

    def test_double_digit_beta_and_rc_are_prereleases(self):
        self.assertTrue(rv.classify("v2.0.0-beta.10", "2.0.0-beta.10")[1])
        self.assertTrue(rv.classify("v2.0.0-rc.1", "2.0.0-rc.1")[1])

    def test_stable_is_not_prerelease(self):
        self.assertEqual(rv.classify("v2.0.0", "2.0.0"), ("2.0.0", False))
        self.assertEqual(rv.classify("v1.2.1", "1.2.1"), ("1.2.1", False))

    def test_tag_must_match_cargo_version(self):
        with self.assertRaisesRegex(rv.ReleaseVersionError, "Cargo.toml"):
            rv.classify("v2.0.0-beta.1", "1.2.1")
        # The pre-release suffix is part of the version: beta.2 is not beta.1.
        with self.assertRaises(rv.ReleaseVersionError):
            rv.classify("v2.0.0-beta.2", "2.0.0-beta.1")

    def test_malformed_tags_are_refused(self):
        for tag in ("2.0.0-beta.1", "v2.0", "v2.0.0-", "v2.0.0-beta.01", "v02.0.0",
                    "v2.0.0+build.5", "v2.0.0-beta.1+build.5", "v2.0.0-beta..1", ""):
            with self.subTest(tag=tag), self.assertRaises(rv.ReleaseVersionError):
                rv.classify(tag, tag[1:] if tag.startswith("v") else tag)

    def test_main_writes_github_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            cargo = pathlib.Path(tmp, "Cargo.toml")
            cargo.write_text('[package]\nname = "x"\nversion = "2.0.0-beta.1"\n')
            out = pathlib.Path(tmp, "out")
            with mock.patch.dict(os.environ, {"GITHUB_OUTPUT": str(out)}), \
                    mock.patch("sys.stdout"):
                rc = rv.main(["--tag", "v2.0.0-beta.1", "--cargo-toml", str(cargo)])
            self.assertEqual(rc, 0)
            self.assertEqual(out.read_text(), "version=2.0.0-beta.1\nprerelease=true\n")

    def test_main_fails_on_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            cargo = pathlib.Path(tmp, "Cargo.toml")
            cargo.write_text('[package]\nname = "x"\nversion = "1.2.1"\n')
            with mock.patch("sys.stderr"), mock.patch("sys.stdout"):
                rc = rv.main(["--tag", "v2.0.0-beta.1", "--cargo-toml", str(cargo)])
            self.assertEqual(rc, 1)

    def test_repo_cargo_version_is_taggable(self):
        """The committed crate version is a valid tag target for its own `v` tag."""
        version = rv.cargo_package_version(REPO / "Cargo.toml")
        self.assertEqual(rv.classify(f"v{version}", version)[0], version)


class ReleaseAssetProvenanceTests(unittest.TestCase):
    VERSION = "2.0.0-beta.1"
    TAG = "v2.0.0-beta.1"
    SHA = "1" * 40
    RUN_ID = "123456"
    RUN_ATTEMPT = "2"
    PAYLOAD_NAMES = (
        "oxidex-x86_64-unknown-linux-musl",
        "oxidex-aarch64-unknown-linux-musl",
        "oxidex-x86_64-pc-windows-gnu.exe",
        "oxidex-universal-apple-darwin",
        "oxidex-v2.0.0-beta.1.dmg",
    )
    SBOM_NAME = "oxidex-v2.0.0-beta.1.sbom.cdx.json"

    @property
    def release_asset_names(self) -> tuple[str, ...]:
        return (*self.PAYLOAD_NAMES, self.SBOM_NAME, "SHA256SUMS")

    def run_assets(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(RELEASE_ASSETS), *args],
            cwd=REPO,
            text=True,
            capture_output=True,
            check=False,
        )

    def create_payloads(self, directory: pathlib.Path) -> None:
        directory.mkdir()
        for index, name in enumerate(self.PAYLOAD_NAMES, start=1):
            (directory / name).write_bytes(f"asset-{index}\n".encode())

    def identity_args(self) -> list[str]:
        return [
            "--version", self.VERSION,
            "--tag", self.TAG,
            "--head-sha", self.SHA,
            "--run-id", self.RUN_ID,
            "--run-attempt", self.RUN_ATTEMPT,
        ]

    def source_args(self) -> list[str]:
        return ["--source-root", str(REPO)]

    def verify_source_args(self, source: pathlib.Path) -> list[str]:
        return ["--source-root", str(source)]

    def test_independent_provenance_verifies_exact_release_assets(self):
        """Removing independent manifest generation must make verification fail."""
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            assets = root / "release-assets"
            provenance = root / "provenance"
            self.create_payloads(assets)

            created = self.run_assets(
                "create-provenance", "--assets", str(assets),
                "--provenance", str(provenance), *self.identity_args(),
                *self.source_args())
            self.assertEqual(created.returncode, 0, created.stderr)
            self.assertEqual(
                sorted(path.name for path in assets.iterdir()),
                sorted((*self.PAYLOAD_NAMES, self.SBOM_NAME, "SHA256SUMS")),
            )

            verified = self.run_assets(
                "verify-assets", "--assets", str(assets),
                "--provenance", str(provenance), *self.identity_args(),
                *self.verify_source_args(REPO))
            self.assertEqual(verified.returncode, 0, verified.stderr)
            self.assertIn("verified 7 exact release assets", verified.stdout)

    def test_sbom_is_deterministic_and_binds_exact_source_and_payload_digests(self):
        """A changed lockfile or payload must not yield the same release SBOM."""
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            assets = root / "release-assets"
            provenance = root / "provenance"
            source = root / "source"
            source.mkdir()
            (source / "Cargo.toml").write_text(
                '[package]\nname = "oxidex"\nversion = "2.0.0-beta.1"\n')
            (source / "Cargo.lock").write_text(
                'version = 4\n\n[[package]]\nname = "alpha"\nversion = "1.0.0"\n')
            self.create_payloads(assets)
            args = ["--source-root", str(source)]

            created = self.run_assets(
                "create-provenance", "--assets", str(assets),
                "--provenance", str(provenance), *self.identity_args(), *args)
            self.assertEqual(created.returncode, 0, created.stderr)
            first = (assets / self.SBOM_NAME).read_bytes()
            sbom = json.loads(first)
            self.assertEqual(sbom["bomFormat"], "CycloneDX")
            self.assertEqual(sbom["metadata"]["component"]["version"], self.VERSION)
            self.assertEqual(
                sbom["metadata"]["properties"],
                [{"name": "org.oxidex.source.cargo_lock.sha256", "value":
                  __import__("hashlib").sha256((source / "Cargo.lock").read_bytes()).hexdigest()}],
            )
            self.assertEqual(
                {component["name"] for component in sbom["components"]},
                {"alpha", *self.PAYLOAD_NAMES},
            )

            (source / "Cargo.lock").write_text(
                'version = 4\n\n[[package]]\nname = "alpha"\nversion = "1.0.1"\n')
            (assets / self.SBOM_NAME).unlink()
            (assets / "SHA256SUMS").unlink()
            provenance_2 = root / "provenance-2"
            created_again = self.run_assets(
                "create-provenance", "--assets", str(assets),
                "--provenance", str(provenance_2), *self.identity_args(), *args)
            self.assertEqual(created_again.returncode, 0, created_again.stderr)
            self.assertNotEqual((assets / self.SBOM_NAME).read_bytes(), first)

            (assets / self.PAYLOAD_NAMES[0]).write_bytes(b"changed payload\n")
            verified = self.run_assets(
                "verify-assets", "--assets", str(assets),
                "--provenance", str(provenance), *self.identity_args(),
                *self.verify_source_args(source))
            self.assertNotEqual(verified.returncode, 0)
            self.assertIn("differs from run provenance", verified.stderr)

    def test_verify_assets_rejects_self_consistent_sbom_semantic_mutations(self):
        """SBOM hashes alone must not make a wrong source/payload inventory pass."""
        for mutation in ("lock-digest", "library-component", "payload-hash"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                assets = root / "release-assets"
                provenance = root / "provenance"
                source = root / "source"
                source.mkdir()
                (source / "Cargo.toml").write_text(
                    '[package]\nname = "oxidex"\nversion = "2.0.0-beta.1"\n')
                (source / "Cargo.lock").write_text(
                    'version = 4\n\n[[package]]\nname = "alpha"\nversion = "1.0.0"\n')
                self.create_payloads(assets)
                created = self.run_assets(
                    "create-provenance", "--assets", str(assets),
                    "--provenance", str(provenance), *self.identity_args(),
                    "--source-root", str(source))
                self.assertEqual(created.returncode, 0, created.stderr)

                sbom_path = assets / self.SBOM_NAME
                sbom = json.loads(sbom_path.read_text())
                if mutation == "lock-digest":
                    sbom["metadata"]["properties"][0]["value"] = "0" * 64
                elif mutation == "library-component":
                    sbom["components"] = [
                        component for component in sbom["components"]
                        if component.get("name") != "alpha"
                    ]
                else:
                    payload = next(component for component in sbom["components"]
                                    if component.get("name") == self.PAYLOAD_NAMES[0])
                    payload["hashes"][0]["content"] = "0" * 64
                sbom_path.write_text(
                    json.dumps(sbom, sort_keys=True, separators=(",", ":")) + "\n")

                manifest_path = provenance / "provenance.json"
                manifest = json.loads(manifest_path.read_text())
                manifest["sbom"]["sha256"] = hashlib.sha256(
                    sbom_path.read_bytes()).hexdigest()
                manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
                checksum_names = (*self.PAYLOAD_NAMES, self.SBOM_NAME)
                checksums = "".join(
                    f"{hashlib.sha256((assets / name).read_bytes()).hexdigest()}  {name}\n"
                    for name in sorted(checksum_names))
                (assets / "SHA256SUMS").write_text(checksums)
                (provenance / "SHA256SUMS").write_text(checksums)

                verified = self.run_assets(
                    "verify-assets", "--assets", str(assets),
                    "--provenance", str(provenance), *self.identity_args(),
                    "--source-root", str(source))
                self.assertNotEqual(verified.returncode, 0)
                self.assertIn("SBOM", verified.stderr)

    def test_verify_assets_rejects_self_rehashed_duplicate_key_sbom(self):
        """Ambiguous duplicate-key JSON must fail before SBOM semantics are checked."""
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            assets = root / "release-assets"
            provenance = root / "provenance"
            self.create_payloads(assets)
            created = self.run_assets(
                "create-provenance", "--assets", str(assets),
                "--provenance", str(provenance), *self.identity_args(),
                *self.source_args())
            self.assertEqual(created.returncode, 0, created.stderr)

            sbom_path = assets / self.SBOM_NAME
            duplicate = sbom_path.read_bytes().replace(
                b'"bomFormat":"CycloneDX"',
                b'"bomFormat":"not-CycloneDX","bomFormat":"CycloneDX"',
                1)
            self.assertNotEqual(duplicate, sbom_path.read_bytes())
            sbom_path.write_bytes(duplicate)

            manifest_path = provenance / "provenance.json"
            manifest = json.loads(manifest_path.read_text())
            manifest["sbom"]["sha256"] = hashlib.sha256(duplicate).hexdigest()
            manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
            checksums = "".join(
                f"{hashlib.sha256((assets / name).read_bytes()).hexdigest()}  {name}\n"
                for name in sorted((*self.PAYLOAD_NAMES, self.SBOM_NAME)))
            (assets / "SHA256SUMS").write_text(checksums)
            (provenance / "SHA256SUMS").write_text(checksums)

            verified = self.run_assets(
                "verify-assets", "--assets", str(assets),
                "--provenance", str(provenance), *self.identity_args(),
                *self.source_args())
            self.assertNotEqual(verified.returncode, 0)
            self.assertIn("duplicate", verified.stderr.lower())

    def test_release_json_binds_exact_id_state_commit_and_asset_set(self):
        """Accepting a different release ID or state must make publication fail."""
        with tempfile.TemporaryDirectory() as tmp:
            release_json = pathlib.Path(tmp, "release.json")
            release_json.write_text(json.dumps({
                "id": 987,
                "tag_name": self.TAG,
                # GitHub documents target_commitish as unused when the tag
                # already exists, so the remote tag is resolved separately.
                "target_commitish": "main",
                "draft": True,
                "prerelease": True,
                "assets": [
                    {"name": name, "size": index}
                    for index, name in enumerate(
                        self.release_asset_names, start=1)
                ],
            }))

            verified = self.run_assets(
                "verify-release", "--release-json", str(release_json),
                "--release-id", "987", "--draft", "true",
                "--prerelease", "true", "--resolved-target", self.SHA,
                *self.identity_args())
            self.assertEqual(verified.returncode, 0, verified.stderr)
            self.assertIn("verified draft release 987", verified.stdout)

    def test_independent_manifest_detects_release_asset_tampering(self):
        """Changing an asset and its co-uploaded checksum must not defeat provenance."""
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            assets = root / "release-assets"
            provenance = root / "provenance"
            self.create_payloads(assets)
            created = self.run_assets(
                "create-provenance", "--assets", str(assets),
                "--provenance", str(provenance), *self.identity_args(),
                *self.source_args())
            self.assertEqual(created.returncode, 0, created.stderr)

            victim = assets / self.PAYLOAD_NAMES[0]
            victim.write_bytes(b"replacement\n")
            digest = __import__("hashlib").sha256(victim.read_bytes()).hexdigest()
            lines = (assets / "SHA256SUMS").read_text().splitlines()
            lines[0] = f"{digest}  {self.PAYLOAD_NAMES[0]}"
            (assets / "SHA256SUMS").write_text("\n".join(lines) + "\n")

            verified = self.run_assets(
                "verify-assets", "--assets", str(assets),
                "--provenance", str(provenance), *self.identity_args(),
                "--source-root", str(REPO))
            self.assertNotEqual(verified.returncode, 0)
            self.assertIn("differs from run provenance", verified.stderr)

    def test_release_verification_rejects_changed_state_id_or_asset_set(self):
        with tempfile.TemporaryDirectory() as tmp:
            release_json = pathlib.Path(tmp, "release.json")
            base = {
                "id": 987, "tag_name": self.TAG, "target_commitish": "main",
                "draft": True, "prerelease": True,
                "assets": [{"name": name, "size": 1}
                           for name in self.release_asset_names],
            }
            for mutation in (
                    {"id": 988}, {"draft": False},
                    {"assets": base["assets"][:-1]},
                    {"assets": [*base["assets"][:-1],
                                {"name": "unexpected", "size": 1}]},
                    {"assets": [{**base["assets"][0], "size": 0}, *base["assets"][1:]]}):
                with self.subTest(mutation=mutation):
                    release_json.write_text(json.dumps({**base, **mutation}))
                    verified = self.run_assets(
                        "verify-release", "--release-json", str(release_json),
                        "--release-id", "987", "--draft", "true",
                        "--prerelease", "true", "--resolved-target", self.SHA,
                        *self.identity_args())
                    self.assertNotEqual(verified.returncode, 0)

            release_json.write_text(json.dumps(base))
            wrong_target = self.run_assets(
                "verify-release", "--release-json", str(release_json),
                "--release-id", "987", "--draft", "true",
                "--prerelease", "true", "--resolved-target", "2" * 40,
                *self.identity_args())
            self.assertNotEqual(wrong_target.returncode, 0)

    def test_published_release_requires_fail_closed_latest_api_evidence(self):
        """Publishing must reject a beta made Latest or a stable not made Latest."""
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            release_json = root / "release.json"
            latest_json = root / "latest.json"
            beta = {
                "id": 987, "tag_name": self.TAG, "draft": False,
                "prerelease": True,
                "assets": [{"name": name, "size": 1}
                           for name in self.release_asset_names],
            }
            release_json.write_text(json.dumps(beta))
            latest_json.write_text(json.dumps({
                "tag_name": "v1.9.0", "draft": False, "prerelease": False,
            }))
            beta_ok = self.run_assets(
                "verify-release", "--release-json", str(release_json),
                "--latest-json", str(latest_json), "--release-id", "987",
                "--draft", "false", "--prerelease", "true",
                "--resolved-target", self.SHA, *self.identity_args())
            self.assertEqual(beta_ok.returncode, 0, beta_ok.stderr)

            latest_json.write_text(json.dumps({"status": 404}))
            beta_without_stable_latest = self.run_assets(
                "verify-release", "--release-json", str(release_json),
                "--latest-json", str(latest_json), "--release-id", "987",
                "--draft", "false", "--prerelease", "true",
                "--resolved-target", self.SHA, *self.identity_args())
            self.assertEqual(beta_without_stable_latest.returncode, 0,
                             beta_without_stable_latest.stderr)

            latest_json.write_text(json.dumps({
                "tag_name": self.TAG, "draft": False, "prerelease": True,
            }))
            beta_latest = self.run_assets(
                "verify-release", "--release-json", str(release_json),
                "--latest-json", str(latest_json), "--release-id", "987",
                "--draft", "false", "--prerelease", "true",
                "--resolved-target", self.SHA, *self.identity_args())
            self.assertNotEqual(beta_latest.returncode, 0)

            stable = {**beta, "tag_name": "v2.0.0", "prerelease": False}
            release_json.write_text(json.dumps(stable))
            stable_latest = self.run_assets(
                "verify-release", "--release-json", str(release_json),
                "--latest-json", str(latest_json), "--release-id", "987",
                "--tag", "v2.0.0", "--draft", "false", "--prerelease", "false",
                "--resolved-target", self.SHA, "--version", "2.0.0",
                "--head-sha", self.SHA, "--run-id", self.RUN_ID,
                "--run-attempt", self.RUN_ATTEMPT)
            self.assertNotEqual(stable_latest.returncode, 0)


class MacOSReleaseVerificationTests(unittest.TestCase):
    def make_executable(self, path: pathlib.Path, text: str) -> None:
        path.write_text(text)
        path.chmod(0o755)

    def make_fake_tools(self, root: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
        fake_bin = root / "fake-bin"
        fake_bin.mkdir()
        tool_log = root / "tools.log"
        common = "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\\n' \"$(basename \"$0\") $*\" >> \"$TOOL_LOG\"\n"
        self.make_executable(
            fake_bin / "lipo", common + "printf '%s\\n' 'aarch64 x86_64'\n")
        self.make_executable(
            fake_bin / "codesign",
            common
            + "if [ \"${1:-}\" = --display ]; then\n"
            + "  printf '%s\\n' 'Identifier=org.oxidex.cli' 'Runtime Version=15.0.0' "
            + "'Timestamp=Sep 19, 2026' 'TeamIdentifier=TEAM123' >&2\n"
            + "fi\n",
        )
        self.make_executable(
            fake_bin / "spctl",
            common
            + "if [ \"${FAIL_PAYLOAD_SPCTL:-0}\" = 1 ] && "
            + "printf '%s\\n' \"$*\" | grep -F 'oxidex-dmg-mount.' >/dev/null; then\n"
            + "  exit 3\n"
            + "fi\n",
        )
        self.make_executable(fake_bin / "xcrun", common)
        self.make_executable(
            fake_bin / "hdiutil",
            common
            + "if [ \"${1:-}\" = attach ]; then\n"
            + "  while [ \"$#\" -gt 0 ]; do\n"
            + "    if [ \"$1\" = -mountpoint ]; then shift; mount=$1; fi\n"
            + "    shift\n"
            + "  done\n"
            + "  cp \"$FAKE_PAYLOAD_SOURCE\" \"$mount/oxidex\"\n"
            + "  chmod +x \"$mount/oxidex\"\n"
            + "elif [ \"${1:-}\" = detach ]; then\n"
            + "  rm -f -- \"$2/oxidex\"\n"
            + "fi\n",
        )
        return fake_bin, tool_log

    def run_verifier(self, fail_payload: bool = False) -> tuple[subprocess.CompletedProcess[str], str]:
        temporary = tempfile.TemporaryDirectory()
        try:
            root = pathlib.Path(temporary.name)
            assets = root / "assets"
            assets.mkdir()
            standalone = assets / "oxidex-universal-apple-darwin"
            self.make_executable(
                standalone,
                "#!/usr/bin/env bash\n"
                "if [ \"${1:-}\" = --version ]; then echo 'oxidex 2.0.0-beta.1'; "
                "elif [ \"${1:-}\" = --help ]; then echo help; fi\n",
            )
            (assets / "oxidex-v2.0.0-beta.1.dmg").write_bytes(b"fake-dmg\n")
            fake_bin, tool_log = self.make_fake_tools(root)
            mount_parent = root / "mount-parent"
            mount_parent.mkdir()
            env = os.environ.copy()
            env.update({
                "ASSET_DIR": str(assets),
                "VERSION": "2.0.0-beta.1",
                "DEVELOPMENT_TEAM": "TEAM123",
                "RUNNER_TEMP": str(mount_parent),
                "TOOL_LOG": str(tool_log),
                "FAKE_PAYLOAD_SOURCE": str(standalone),
                "PATH": f"{fake_bin}:{env['PATH']}",
            })
            if fail_payload:
                env["FAIL_PAYLOAD_SPCTL"] = "1"
            result = subprocess.run(
                ["bash", str(VERIFY_MACOS_RELEASE)], cwd=REPO, env=env,
                text=True, capture_output=True, check=False)
            calls = tool_log.read_text() if tool_log.exists() else ""
            return result, calls
        finally:
            temporary.cleanup()

    def test_real_verifier_assesses_both_executables_and_cleans_mount(self):
        """Removing either execute assessment or detach must fail this behavior test."""
        result, calls = self.run_verifier()
        self.assertEqual(result.returncode, 0, result.stderr)
        execute_assessments = [
            line for line in calls.splitlines()
            if line.startswith("spctl --assess --type execute")
        ]
        self.assertEqual(len(execute_assessments), 2, calls)
        self.assertIn("xcrun stapler validate -v", calls)
        self.assertIn("hdiutil detach", calls)
        self.assertLess(calls.index("xcrun stapler validate -v"),
                        calls.index("hdiutil attach"))
        self.assertLess(calls.index("spctl --assess --type open"),
                        calls.index("hdiutil attach"))

    def test_verifier_detaches_after_post_mount_failure(self):
        result, calls = self.run_verifier(fail_payload=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("hdiutil attach", calls)
        self.assertIn("hdiutil detach", calls)


class ReleaseWorkflowTests(unittest.TestCase):
    text = (WORKFLOWS / "release.yml").read_text()

    def test_no_hard_coded_prerelease_false(self):
        self.assertNotRegex(self.text, r"(?m)^\s*prerelease:\s*false\s*$")

    def test_verify_version_classifies_the_tag(self):
        block = job_block(self.text, "verify-version")
        self.assertIn("python3 tools/ci/release_version.py", block)
        self.assertIn("--tag \"$GITHUB_REF_NAME\"", block)
        self.assertIn("prerelease: ${{ steps.classify.outputs.prerelease }}", block)

    def test_every_build_waits_for_verify_version(self):
        for job in ("build-linux", "build-windows", "build-macos", "create-release"):
            with self.subTest(job=job):
                self.assertIn("verify-version", job_needs(job_block(self.text, job)))

    def test_github_release_is_prerelease_and_not_latest_for_dash_tags(self):
        block = job_block(self.text, "create-release")
        self.assertIn('PRERELEASE: ${{ needs.verify-version.outputs.prerelease }}', block)
        self.assertIn('--field "prerelease=$PRERELEASE"', block)
        self.assertIn('--raw-field "make_latest=$MAKE_LATEST"', block)

    def test_stable_docs_are_not_relabelled_by_a_prerelease(self):
        block = job_block(self.text, "update-docs")
        self.assertIn("verify-version", job_needs(block))
        self.assertRegex(
            block, r"(?m)^    if: needs\.verify-version\.outputs\.prerelease != 'true'\s*$")

    def test_release_refuses_tags_not_reachable_from_origin_main(self):
        block = job_block(self.text, "verify-version")
        self.assertIn('git fetch --no-tags origin main', block)
        self.assertIn('git merge-base --is-ancestor "$GITHUB_SHA" origin/main', block)

    def test_macos_release_builds_and_asserts_a_universal_binary(self):
        block = job_block(self.text, "build-macos")
        self.assertIn('targets: aarch64-apple-darwin,x86_64-apple-darwin', block)
        self.assertIn('cargo build --release --target aarch64-apple-darwin', block)
        self.assertIn('cargo build --release --target x86_64-apple-darwin', block)
        self.assertIn('lipo -create -output "$APP_PATH"', block)
        self.assertIn('lipo -archs "$APP_PATH"', block)
        self.assertIn('aarch64 x86_64', block)

    def test_universal_build_uses_only_explicit_target_outputs(self):
        block = job_block(self.text, "build-macos")
        self.assertIn('target/aarch64-apple-darwin/release/oxidex', block)
        self.assertIn('target/x86_64-apple-darwin/release/oxidex', block)
        self.assertNotIn('target/release', block)

    def test_gatekeeper_assesses_stapled_dmg_and_both_downloaded_executables(self):
        block = job_block(self.text, "build-macos")
        self.assertNotIn('spctl --assess --type execute', block)
        staple = block.index('xcrun stapler staple "$DMG_PATH"')
        validate = block.index('xcrun stapler validate "$DMG_PATH"')
        assess = block.index('spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG_PATH"')
        self.assertLess(staple, validate)
        self.assertLess(validate, assess)
        verify = job_block(self.text, "verify-release-assets")
        self.assertIn('bash tools/ci/verify_macos_release.sh', verify)

    def test_macos_signing_and_notarization_are_strict_and_fail_closed(self):
        block = job_block(self.text, "build-macos")
        for secret in (
                "BUILD_CERTIFICATE_BASE64", "P12_PASSWORD", "KEYCHAIN_PASSWORD",
                "DEVELOPMENT_TEAM", "NOTARIZATION_APPLE_ID", "NOTARIZATION_PASSWORD",
                "NOTARIZATION_TEAM_ID"):
            with self.subTest(secret=secret):
                self.assertIn(secret, block)
        self.assertIn('expected exactly one Developer ID Application identity', block)
        self.assertIn('codesign --verify --strict --verbose=4 "$APP_PATH"', block)
        self.assertIn('codesign -d --entitlements :- "$APP_PATH"', block)
        self.assertIn('spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG_PATH"', block)
        self.assertIn('xcrun stapler validate "$DMG_PATH"', block)
        self.assertIn('spctl --assess --type open --context context:primary-signature --verbose=4 "$DMG_PATH"', block)
        self.assertIn("awk '!found && /^[[:space:]]+id:/ { print $2; found=1 }'", block)

    def test_macos_signing_identity_is_bound_to_declared_team(self):
        block = job_block(self.text, "build-macos")
        self.assertIn('grep -F "($DEVELOPMENT_TEAM)"', block)

    def test_untrusted_build_precedes_certificate_import_and_import_is_scoped(self):
        block = job_block(self.text, "build-macos")
        self.assertLess(block.index("Build universal release binary"),
                        block.index("Import signing certificate and sign binary"))
        self.assertNotIn("security import \"$CERTIFICATE_PATH\" -P \"$P12_PASSWORD\" -A", block)
        self.assertIn('-x -T /usr/bin/codesign', block)
        self.assertIn('umask 077', block)
        self.assertNotIn("security list-keychain", block)
        self.assertIn('security delete-keychain "$KEYCHAIN_PATH"', block)
        self.assertIn('rm -f -- "$CERTIFICATE_PATH"', block)

    def test_release_assets_have_a_checksum_manifest(self):
        block = job_block(self.text, "create-release")
        self.assertIn('python3 tools/ci/release_assets.py create-provenance', block)
        self.assertIn('--source-root "$GITHUB_WORKSPACE"', block)
        self.assertIn('sbom.cdx.json', block)
        self.assertIn('name: release-provenance', block)
        self.assertLess(block.index('name: Upload independent release provenance'),
                        block.index('name: Create fail-closed draft release'))

    def test_every_actions_artifact_is_scoped_to_exact_run_attempt(self):
        self.assertIn(
            'name: oxidex-${{ matrix.target }}-${{ github.run_id }}-${{ github.run_attempt }}',
            self.text)
        self.assertIn(
            'name: release-provenance-${{ github.run_id }}-${{ github.run_attempt }}',
            self.text)
        for job in ("create-release", "verify-release-assets", "publish-release"):
            block = job_block(self.text, job)
            with self.subTest(job=job):
                self.assertIn('${{ github.run_id }}', block)
                self.assertIn('${{ github.run_attempt }}', block)

    def test_macos_builder_has_no_release_write_permission(self):
        block = job_block(self.text, "build-macos")
        self.assertRegex(block, r"(?m)^    permissions:\n      contents: read$")

    def test_downloaded_release_assets_are_verified_before_publication(self):
        verify = job_block(self.text, "verify-release-assets")
        publish = job_block(self.text, "publish-release")
        self.assertIn("create-release", job_needs(verify))
        self.assertIn('run-id: ${{ github.run_id }}', verify)
        self.assertIn('python3 tools/ci/release_assets.py verify-assets', verify)
        self.assertIn('python3 tools/ci/release_assets.py verify-release', verify)
        self.assertIn('--source-root "$GITHUB_WORKSPACE"', verify)
        self.assertIn('gh api "repos/$GITHUB_REPOSITORY/commits/$GITHUB_REF_NAME"', verify)
        self.assertIn('--resolved-target "$RESOLVED_TARGET"', verify)
        self.assertIn('gh release download "$GITHUB_REF_NAME" --repo "$GITHUB_REPOSITORY"', verify)
        self.assertIn('bash tools/ci/verify_macos_release.sh', verify)
        self.assertIn("verify-release-assets", job_needs(publish))
        self.assertIn('python3 tools/ci/release_assets.py verify-assets', publish)
        self.assertIn('python3 tools/ci/release_assets.py verify-release', publish)
        self.assertIn('--source-root "$GITHUB_WORKSPACE"', publish)
        self.assertIn('--method PATCH "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID"', publish)

    def test_published_release_verification_reads_fail_closed_latest_api_evidence(self):
        publish = job_block(self.text, "publish-release")
        self.assertIn('repos/$GITHUB_REPOSITORY/releases/latest', publish)
        self.assertIn('--latest-json latest-release.json', publish)

    def run_latest_poll(self, mode: str, prerelease: bool, max_attempts: int = 3):
        script = step_run(job_block(self.text, "publish-release"),
                          "Poll Latest release projection")
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            count_file = root / "count"
            gh = fake_bin / "gh"
            gh.write_text(
                "#!/usr/bin/env bash\n"
                "set -euo pipefail\n"
                "count=0; test -f \"$COUNT_FILE\" && count=$(cat \"$COUNT_FILE\")\n"
                "count=$((count + 1)); printf '%s' \"$count\" > \"$COUNT_FILE\"\n"
                "include=0; printf '%s\\n' \"$*\" | grep -F -- '--include' >/dev/null && include=1 || true\n"
                "emit() { if [ \"$include\" -eq 1 ]; then printf 'HTTP/2.0 %s\\n\\n%s\\n' \"$1\" \"$2\"; else printf '%s\\n' \"$2\"; fi; }\n"
                "case \"$POLL_MODE\" in\n"
                "  stale-then-current) if [ \"$count\" -eq 1 ]; then "
                "emit 200 '{\"tag_name\":\"v1.9.0\",\"draft\":false,\"prerelease\":false}'; "
                "else emit 200 '{\"tag_name\":\"v2.0.0\",\"draft\":false,\"prerelease\":false}'; fi;;\n"
                "  stale) emit 200 '{\"tag_name\":\"v1.9.0\",\"draft\":false,\"prerelease\":false}';;\n"
                "  not-found-then-current) if [ \"$count\" -eq 1 ]; then "
                "if [ \"$include\" -eq 1 ]; then emit 404 '{\"message\":\"Not Found\"}'; else "
                "echo 'HTTP 404: Not Found' >&2; fi; exit 1; "
                "else emit 200 '{\"tag_name\":\"v1.9.0\",\"draft\":false,\"prerelease\":false}'; fi;;\n"
                "  not-found) if [ \"$include\" -eq 1 ]; then emit 404 '{\"message\":\"Not Found\"}'; else "
                "echo 'HTTP 404: Not Found' >&2; fi; exit 1;;\n"
                "  server-error-not-found) if [ \"$include\" -eq 1 ]; then emit 500 '{\"message\":\"Not Found\"}'; else "
                "echo 'HTTP 500: Not Found' >&2; fi; exit 1;;\n"
                "esac\n")
            gh.chmod(0o755)
            env = os.environ.copy()
            env.update({
                "PATH": f"{fake_bin}:{env['PATH']}",
                "COUNT_FILE": str(count_file),
                "POLL_MODE": mode,
                "LATEST_MAX_ATTEMPTS": str(max_attempts),
                "LATEST_POLL_DELAY": "0",
                "PRERELEASE": "true" if prerelease else "false",
                "GITHUB_REF_NAME": "v2.0.0-beta.1" if prerelease else "v2.0.0",
                "GITHUB_REPOSITORY": "owner/repo",
            })
            result = subprocess.run(
                ["bash", "-euo", "pipefail", "-c", script], cwd=root, env=env,
                text=True, capture_output=True, check=False)
            latest = (root / "latest-release.json").read_text() \
                if (root / "latest-release.json").exists() else ""
            history = (root / "latest-release-attempts.jsonl").read_text() \
                if (root / "latest-release-attempts.jsonl").exists() else ""
            count = int(count_file.read_text()) if count_file.exists() else 0
            return result, latest, history, count

    def test_latest_poll_retries_stale_projection_until_current(self):
        result, latest, history, count = self.run_latest_poll("stale-then-current", False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(count, 2)
        self.assertEqual(json.loads(latest)["tag_name"], "v2.0.0")
        self.assertEqual(len(history.splitlines()), 2)

    def test_latest_poll_fails_after_bounded_stale_projection(self):
        result, _latest, history, count = self.run_latest_poll("stale", False, max_attempts=2)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(count, 2)
        self.assertEqual(len(history.splitlines()), 2)

    def test_latest_poll_accepts_prerelease_without_any_stable_release(self):
        result, latest, history, count = self.run_latest_poll("not-found", True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(count, 3)
        self.assertEqual(json.loads(latest), {"status": 404})
        self.assertEqual(len(history.splitlines()), 3)

    def test_latest_poll_retries_404_until_a_later_stable_latest(self):
        result, latest, history, count = self.run_latest_poll(
            "not-found-then-current", True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(count, 2)
        self.assertEqual(json.loads(latest)["tag_name"], "v1.9.0")
        self.assertEqual(len(history.splitlines()), 2)

    def test_latest_poll_rejects_500_even_when_error_text_says_not_found(self):
        result, _latest, history, count = self.run_latest_poll(
            "server-error-not-found", True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(count, 1)
        self.assertEqual(history, "")

    def test_release_is_bound_to_exact_id_and_fails_closed_on_rerun(self):
        create = job_block(self.text, "create-release")
        self.assertIn('outputs:', create)
        self.assertIn('release-id: ${{ steps.create-draft.outputs.release_id }}', create)
        self.assertIn('--method POST "repos/$GITHUB_REPOSITORY/releases"', create)
        self.assertNotIn('softprops/action-gh-release', create)
        self.assertNotIn('--clobber', create)
        self.assertIn('A failed run may leave a draft', create)

    def test_release_commands_have_explicit_repository_context(self):
        for line in self.text.splitlines():
            stripped = line.strip()
            if stripped.startswith("gh release "):
                with self.subTest(line=stripped):
                    self.assertIn('--repo "$GITHUB_REPOSITORY"', stripped)
            if stripped.startswith("gh api "):
                with self.subTest(line=stripped):
                    self.assertIn('repos/$GITHUB_REPOSITORY/', stripped)

    def test_least_privilege_and_per_tag_serialization(self):
        prefix = self.text.split("jobs:", 1)[0]
        self.assertRegex(prefix, r"(?m)^permissions:\n  contents: read\n  actions: read$")
        self.assertIn("group: release-${{ github.ref }}", prefix)
        self.assertIn("cancel-in-progress: false", prefix)
        for job in ("create-release", "publish-release"):
            with self.subTest(job=job):
                block = job_block(self.text, job)
                self.assertRegex(
                    block, r"(?m)^    permissions:\n      contents: write\n      actions: read$")

    def test_existing_release_api_failure_stops_draft_creation(self):
        script = step_run(job_block(self.text, "create-release"),
                          "Create fail-closed draft release")
        script = script.replace(
            "${{ steps.version.outputs.version }}", "2.0.0-beta.1")
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "release-notes.md").write_text("notes\n")
            fake_bin = root / "bin"
            fake_bin.mkdir()
            log = root / "gh.log"
            gh = fake_bin / "gh"
            gh.write_text(
                "#!/usr/bin/env bash\n"
                "printf '%s\\n' \"$*\" >> \"$GH_LOG\"\n"
                "echo 'HTTP 422: already_exists' >&2\n"
                "exit 1\n")
            gh.chmod(0o755)
            output = root / "github-output"
            env = os.environ.copy()
            env.update({
                "PATH": f"{fake_bin}:{env['PATH']}", "GH_LOG": str(log),
                "PRERELEASE": "true", "GITHUB_REF_NAME": "v2.0.0-beta.1",
                "GITHUB_SHA": "1" * 40, "GITHUB_REPOSITORY": "owner/repo",
                "GITHUB_OUTPUT": str(output),
            })
            result = subprocess.run(
                ["bash", "-euo", "pipefail", "-c", script], cwd=root, env=env,
                text=True, capture_output=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("--method POST repos/owner/repo/releases", log.read_text())
            self.assertFalse(output.exists(), "failed rerun must not emit a release ID")

    def test_signing_cleanup_removes_p12_and_keychain_on_codesign_failure(self):
        script = step_run(job_block(self.text, "build-macos"),
                          "Import signing certificate and sign binary")
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "artifacts").mkdir()
            (root / "artifacts" / "oxidex-universal-apple-darwin").write_bytes(b"binary")
            runner_temp = root / "runner-temp"
            runner_temp.mkdir()
            fake_bin = root / "bin"
            fake_bin.mkdir()
            log = root / "tools.log"
            security = fake_bin / "security"
            security.write_text(
                "#!/usr/bin/env bash\nset -u\nprintf 'security %s\\n' \"$*\" >> \"$TOOL_LOG\"\n"
                "case \"${1:-}\" in\n"
                "  create-keychain) touch \"${!#}\";;\n"
                "  find-identity) echo '  1) ABCDEF \"Developer ID Application: Test (TEAM123)\"';;\n"
                "  delete-keychain) rm -f -- \"${!#}\";;\n"
                "esac\n")
            security.chmod(0o755)
            codesign = fake_bin / "codesign"
            codesign.write_text(
                "#!/usr/bin/env bash\nset -u\nprintf 'codesign %s\\n' \"$*\" >> \"$TOOL_LOG\"\n"
                "if [ \"${1:-}\" = --sign ]; then exit 9; fi\n")
            codesign.chmod(0o755)
            base64 = fake_bin / "base64"
            base64.write_text(
                "#!/usr/bin/env bash\nset -eu\n"
                "test \"${1:-}\" = --decode\n"
                "test \"${2:-}\" = -o\n"
                "test -n \"${3:-}\"\n"
                "cat > \"$3\"\n")
            base64.chmod(0o755)
            env = os.environ.copy()
            env.update({
                "PATH": f"{fake_bin}:{env['PATH']}", "TOOL_LOG": str(log),
                "RUNNER_TEMP": str(runner_temp),
                "BUILD_CERTIFICATE_BASE64": "ZmFrZQ==", "P12_PASSWORD": "p12",
                "KEYCHAIN_PASSWORD": "keychain", "DEVELOPMENT_TEAM": "TEAM123",
            })
            result = subprocess.run(
                ["bash", "-euo", "pipefail", "-c", script], cwd=root, env=env,
                text=True, capture_output=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((runner_temp / "build_certificate.p12").exists())
            self.assertFalse((runner_temp / "app-signing.keychain-db").exists())
            self.assertIn("security delete-keychain", log.read_text())


class DockerWorkflowTests(unittest.TestCase):
    text = (WORKFLOWS / "docker.yml").read_text()
    GATE = "enable=${{ !contains(github.ref_name, '-') }}"

    def meta_step(self) -> str:
        block = job_block(self.text, "merge")
        match = re.search(r"- name: Compute tags\n(.*?)(?=\n      - name: |\Z)", block, re.S)
        self.assertIsNotNone(match, "Compute tags step not found in the merge job")
        return match.group(1)

    def test_metadata_action_auto_latest_is_off(self):
        self.assertRegex(self.meta_step(), r"(?m)^\s*latest=false\s*$")

    def test_every_floating_tag_is_stable_only(self):
        tag_lines = [line.strip() for line in self.meta_step().splitlines()
                     if line.strip().startswith("type=")]
        self.assertTrue(tag_lines, "no tag rules found")
        floating = [line for line in tag_lines
                    if "value=latest" in line or "type=edge" in line
                    or re.search(r"pattern=\{\{(major|minor)", line)]
        self.assertTrue(floating, "expected an explicit :latest rule")
        for line in floating:
            with self.subTest(rule=line):
                self.assertIn(self.GATE, line)

    def test_exact_version_tags_still_publish(self):
        step = self.meta_step()
        self.assertIn("type=ref,event=tag", step)
        self.assertIn("type=semver,pattern={{version}}", step)


class TagRecipeTests(unittest.TestCase):
    text = (REPO / "justfile").read_text()
    SHA = "1" * 40

    def run_rendered_tag(self, *, authorization: str = "", dry_run: bool = False):
        rendered = subprocess.run(
            ["just", "--dry-run", "tag", "2.0.0-beta.1", self.SHA], cwd=REPO,
            text=True, capture_output=True, check=False)
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            log = root / "calls.log"
            fake_bin = root / "bin"
            fake_bin.mkdir()
            git = fake_bin / "git"
            git.write_text(
                "#!/usr/bin/env bash\nset -u\nprintf 'git %s\\n' \"$*\" >> \"$CALL_LOG\"\n"
                "case \"$*\" in\n"
                "  'rev-parse -q --verify refs/tags/'*) exit 1;;\n"
                "  'rev-parse --verify '*'^{commit}') echo \"$EXPECTED_SHA\";;\n"
                "  'merge-base --is-ancestor '*) exit 0;;\n"
                "  'show '*) echo 'version = \"2.0.0-beta.1\"';;\n"
                "  'config --get gpg.format') echo ssh;;\n"
                "  'config --get user.signingkey') echo 'ssh-ed25519 AAAATEST';;\n"
                "  'config --get user.email') echo 'release@example.invalid';;\n"
                "  *) exit 0;;\n"
                "esac\n")
            git.chmod(0o755)
            gh = fake_bin / "gh"
            gh.write_text(
                "#!/usr/bin/env bash\nset -u\nprintf 'gh %s\\n' \"$*\" >> \"$CALL_LOG\"\n"
                "case \"$*\" in\n"
                "  'repo view '*) echo owner/repo;;\n"
                "  'api '*) echo true;;\n"
                "esac\n")
            gh.chmod(0o755)
            env = os.environ.copy()
            env.update({
                "PATH": f"{fake_bin}:{env['PATH']}",
                "CALL_LOG": str(log), "EXPECTED_SHA": self.SHA,
                "OXIDEX_TAG_AUTHORIZATION": authorization,
                "OXIDEX_TAG_DRY_RUN": "1" if dry_run else "0",
            })
            result = subprocess.run(
                ["bash", "-c", rendered.stderr], cwd=root, env=env,
                text=True, capture_output=True, check=False)
            return result, log.read_text() if log.exists() else ""

    def test_tag_recipe_requires_main_and_exact_authorization(self):
        recipe = self.text[
            self.text.index("tag version"):
            self.text.index("# macOS packaging", self.text.index("tag version"))]
        self.assertIn('commit="origin/main"', recipe)
        self.assertIn('git merge-base --is-ancestor "$SHA" origin/main', recipe)
        self.assertNotIn('origin/refactor/tag-machinery', recipe)
        self.assertIn('OXIDEX_TAG_AUTHORIZATION', recipe)
        self.assertIn('"$TAG@$SHA"', recipe)
        self.assertIn('OXIDEX_TAG_DRY_RUN', recipe)

    def test_remote_tag_deletion_recipe_is_absent(self):
        listed = subprocess.run(
            ["just", "--list"], cwd=REPO, text=True, capture_output=True, check=False)
        self.assertEqual(listed.returncode, 0, listed.stderr)
        self.assertNotRegex(listed.stdout, r"(?m)^\s+untag\b")

    def test_missing_exact_authorization_fails_before_tag_creation_or_push(self):
        result, calls = self.run_rendered_tag()
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("git tag -s", calls)
        self.assertNotIn("git push", calls)

    def test_dry_run_signs_and_verifies_but_never_pushes(self):
        result, calls = self.run_rendered_tag(dry_run=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("git tag -s", calls)
        self.assertIn("tag -v", calls)
        self.assertNotIn("git push", calls)

    def test_exact_authorization_signs_verifies_then_pushes_non_forcing_ref(self):
        authorization = f"v2.0.0-beta.1@{self.SHA}"
        result, calls = self.run_rendered_tag(authorization=authorization)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertLess(calls.index("git tag -s"), calls.index("tag -v"))
        self.assertLess(calls.index("tag -v"), calls.index("git push"))
        self.assertIn("git push origin refs/tags/v2.0.0-beta.1", calls)


class ReleaseGuideTests(unittest.TestCase):
    text = RELEASE_GUIDE.read_text()

    def test_tag_instructions_use_only_the_guarded_main_recipe(self):
        section = self.text[
            self.text.index("## Tag and publish"):
            self.text.index("### What the push triggers")]
        self.assertNotIn("git tag -s", section)
        self.assertNotRegex(section, r"git push .*refs/tags")
        self.assertNotIn("origin/refactor/tag-machinery", section)
        self.assertIn('SHA=$(git rev-parse origin/main)', section)
        self.assertIn('OXIDEX_TAG_AUTHORIZATION="v2.0.0-beta.1@$SHA"', section)
        self.assertIn('just tag 2.0.0-beta.1 "$SHA"', section)

    def test_checklist_and_docker_policy_require_a_main_tag(self):
        self.assertNotIn("branch refactor/tag-machinery", self.text)
        self.assertNotIn("not merged into `main`, so for this tag", self.text)
        self.assertIn("reachable from `origin/main`", self.text)
        self.assertIn(":v2.0.0-beta.1", self.text)
        self.assertIn(":2.0.0-beta.1", self.text)
        self.assertIn("never moves `:latest`", self.text)


class CratesIoTests(unittest.TestCase):
    """No tag push may reach crates.io while the root crate's name is unresolved.

    The crates.io name `oxidex` belongs to another account. A publish step
    run from a tag would upload the eight tag crates and then fail on the
    root one, leaving a half-published release that can't be withdrawn
    (crates.io only yanks). Publishing stays a manual, maintainer-run step
    (docs/RELEASE-2.0.0-beta.1.md) until the name is settled. Lift these
    two guards together, in the same change that settles the name.
    """

    def test_no_workflow_publishes_to_crates_io(self):
        for wf in sorted(WORKFLOWS.glob("*.y*ml")):
            with self.subTest(workflow=wf.name):
                self.assertNotRegex(wf.read_text(), r"cargo\s+publish\b(?![^\n]*--dry-run)")

    def test_root_crate_is_not_publishable(self):
        with (REPO / "Cargo.toml").open("rb") as fh:
            self.assertIs(tomllib.load(fh)["package"].get("publish"), False)


if __name__ == "__main__":
    unittest.main()
