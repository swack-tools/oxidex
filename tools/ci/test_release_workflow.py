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
import os
import pathlib
import re
import sys
import tempfile
import tomllib
import unittest
from unittest import mock

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parent.parent
WORKFLOWS = REPO / ".github" / "workflows"

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
        self.assertIn(
            "prerelease: ${{ needs.verify-version.outputs.prerelease == 'true' }}", block)
        self.assertIn(
            "make_latest: ${{ needs.verify-version.outputs.prerelease == 'true'"
            " && 'false' || 'true' }}", block)

    def test_stable_docs_are_not_relabelled_by_a_prerelease(self):
        block = job_block(self.text, "update-docs")
        self.assertIn("verify-version", job_needs(block))
        self.assertRegex(
            block, r"(?m)^    if: needs\.verify-version\.outputs\.prerelease != 'true'\s*$")


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
