"""The hosted docs caller must not enter the maintainer-only release recipe."""

from pathlib import Path
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from contextlib import contextmanager
from unittest import mock

from tools.ci import docs_comparison


REPO = Path(__file__).resolve().parents[2]


@contextmanager
def isolated_roots():
    """Give this test a durable root and an external sibling it owns."""
    with tempfile.TemporaryDirectory(prefix="docs-test-roots-", dir=REPO.parent) as case:
        root = Path(case)
        durable = root / "durable"
        external = root / "external"
        durable.mkdir()
        external.mkdir()
        yield durable, external


class DocsComparisonRecipeTests(unittest.TestCase):
    def test_hosted_docs_caller_reaches_portable_builder(self):
        workflow = (REPO / ".github/workflows/deploy-docs.yml").read_text()
        command = re.search(
            r"- name: Generate ExifTool comparison report\n\s+run: (.+)", workflow
        ).group(1)
        with isolated_roots() as (durable, external):
            cache = external / "runner-cache"
            environment = {**os.environ, "EXIFTOOL_CACHE_DIR": str(cache),
                           "OXIDEX_OPS_DIR": str(durable),
                           # Keep the subprocess resolver's temporary-root
                           # classifier independent of the caller's hostile
                           # TMPDIR while retaining the owned sibling topology.
                           "TMPDIR": str(external)}
            environment.pop("EXIFTOOL_SOURCE", None)
            result = subprocess.run(
                ["bash", "-c", command], cwd=REPO, env=environment,
                text=True, capture_output=True, timeout=30,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("EXIFTOOL_SOURCE is required", result.stderr)
            self.assertNotIn("outside durable root", result.stderr)

    def test_generator_builds_with_exact_oracle_and_retains_reports(self):
        with isolated_roots() as (durable, external):
            repo = external / "repo"
            repo.mkdir()
            source = repo / "source's tree"
            cache = repo / "runner cache"
            target = repo / "separate target"
            (source / "lib").mkdir(parents=True)
            (source / "t/images").mkdir(parents=True)
            cache.mkdir()
            (target / "release").mkdir(parents=True)
            (repo / ".exiftool-version").write_text("13.59\n")
            (source / "t/images/OOXML.docx").write_bytes(b"docx fixture")
            (source / "exiftool").write_text(
                "die 'configuration enabled' unless shift(@ARGV) eq '-config' && shift(@ARGV) eq '';\n"
                "print $ARGV[0] eq '-ver' ? qq(13.59\\n) : qq(DOCX\\n);\n"
            )
            archive = cache / "samples-Canon.tar.gz"
            with tarfile.open(archive, "w:gz") as output:
                member = tarfile.TarInfo("Canon/sample.jpg")
                member.size = 6
                output.addfile(member, io.BytesIO(b"sample"))
            expected = repo / "expected-corpus"
            (expected / "Canon").mkdir(parents=True)
            (expected / "Canon/sample.jpg").write_bytes(b"sample")
            (expected / "OOXML.docx").write_bytes(b"docx fixture")
            lock = {"exiftool": {"version": "13.59"}, "archives": {
                "samples_canon": {"filename": archive.name, "url": "https://invalid.invalid/sample",
                                  "sha256": docs_comparison.bootstrap.sha256_file(archive)}},
                    "corpus_tree_sha256": docs_comparison.bootstrap.sha256_tree(expected)}
            capture = repo / "comparison-boundary.json"
            binary = target / "release/tag-comparison"
            binary.write_text(
                f"#!{sys.executable}\n"
                "import json, os, pathlib, subprocess, sys\n"
                "args = sys.argv[1:]\n"
                "get = lambda flag: args[args.index(flag) + 1]\n"
                "oracle = get('--exiftool')\n"
                "assert subprocess.check_output([oracle, '-ver'], text=True).strip() == '13.59'\n"
                "pathlib.Path(os.environ['DOCS_TEST_CAPTURE']).write_text(json.dumps({'args': args, 'perl': os.environ['EXIFTOOL_PERL'], 'home': os.environ['EXIFTOOL_HOME'], 'override': os.environ.get('EXIFTOOL')}))\n"
                "pathlib.Path(get('--output')).write_text('{\"current\": true}')\n"
                "pathlib.Path(get('--markdown-dir'), 'index.md').write_text('Current comparison\\n')\n"
            )
            binary.chmod(0o755)
            perl = shutil.which("perl")
            self.assertIsNotNone(perl)
            real_run = subprocess.run
            builds = []

            def external_boundary(argv, **kwargs):
                if argv[0] == "cargo":
                    builds.append(argv)
                    return subprocess.CompletedProcess(argv, 0)
                if "-MArchive::Zip" in argv:
                    return subprocess.CompletedProcess(argv, 0)
                return real_run(argv, **kwargs)

            with mock.patch.dict(os.environ, {
                "EXIFTOOL_SOURCE": str(source), "EXIFTOOL_CACHE_DIR": str(cache),
                "OXIDEX_TABLES_PERL": perl, "CARGO_TARGET_DIR": str(target),
                "DOCS_TEST_CAPTURE": str(capture), "EXIFTOOL": "/untrusted/oracle",
                "OXIDEX_OPS_DIR": str(durable),
            }), mock.patch.object(docs_comparison.bootstrap, "DURABLE_ROOT", durable), mock.patch.object(
                docs_comparison.bootstrap, "LOCK", lock), mock.patch.object(
                docs_comparison.bootstrap, "MIN_CORPUS_FILES", 2
            ), mock.patch.object(docs_comparison.subprocess, "run", side_effect=external_boundary):
                docs_comparison.generate(repo)
            self.assertEqual(builds, [["cargo", "build", "--release", "--bin", "tag-comparison",
                                      "--features", "tag-comparison-binary"]])
            report = repo / "docs/reference/comparison"
            self.assertEqual(json.loads((report / "comparison.json").read_text()), {"current": True})
            call = json.loads(capture.read_text())
            self.assertEqual(call["perl"], perl)
            self.assertEqual(call["home"], os.devnull)
            self.assertIsNone(call["override"])
            self.assertEqual(call["args"][call["args"].index("--output") + 1], str(report / "comparison.json"))
            self.assertEqual(call["args"][call["args"].index("--exiftool-version") + 1], "13.59")
            self.assertFalse(list(cache.glob("docs-comparison-*")))


if __name__ == "__main__":
    unittest.main()
