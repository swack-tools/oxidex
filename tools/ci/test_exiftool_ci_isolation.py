"""Run the committed ExifTool acquisition shell blocks with disposable sources."""

from __future__ import annotations

import os
from concurrent.futures import ThreadPoolExecutor
import subprocess
import tarfile
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CI = ROOT / ".github/workflows/ci.yml"
ACTION = ROOT / ".github/actions/pinned-exiftool/action.yml"


def script_for(text: str, name: str, indent: int) -> str:
    lines = text.splitlines()
    marker = " " * indent + f"- name: {name}"
    start = lines.index(marker)
    run = next(i for i in range(start + 1, len(lines))
               if lines[i] == " " * (indent + 2) + "run: |")
    body = []
    for line in lines[run + 1:]:
        if line.strip() and len(line) - len(line.lstrip()) < indent + 4:
            break
        body.append(line)
    return textwrap.dedent("\n".join(body)) + "\n"


class ExifToolCiIsolationTests(unittest.TestCase):
    def fixture(self, root: Path):
        source = root / "archive-source" / "Image-ExifTool-13.59"
        (source / "t/images").mkdir(parents=True)
        (source / "t/images/OOXML.docx").write_text("sample")
        exiftool = source / "exiftool"
        exiftool.write_text("#!/bin/sh\n"
                            "case \"$1\" in -ver) echo 13.59 ;; -s3) echo DOCX ;; *) exit 1 ;; esac\n")
        exiftool.chmod(0o755)
        archive = root / "source.tar.gz"
        with tarfile.open(archive, "w:gz") as out:
            out.add(source, arcname=source.name)
        fake = root / "fake-bin"
        fake.mkdir()
        curl = fake / "curl"
        curl.write_text("#!/bin/sh\n"
                        "printf 'called\\n' >> \"${FAKE_CURL_LOG:-/dev/null}\"\n"
                        "while [ $# -gt 0 ]; do\n"
                        "  case \"$1\" in -o|--output) shift; cp \"$FAKE_ARCHIVE\" \"$1\"; exit $? ;; esac\n"
                        "  shift\n"
                        "done\nexit 99\n")
        curl.chmod(0o755)
        for name in ("sudo", "perl"):
            stub = fake / name
            stub.write_text("#!/bin/sh\nexit 0\n")
            stub.chmod(0o755)
        return archive, fake

    def run_block(self, script: str, env: dict):
        result = subprocess.run(["bash", "-c", script], env=env,
                                text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_build_test_cache_hit_and_miss_have_clean_job_source(self):
        text = CI.read_text()
        test_job = text.split("  test:\n", 1)[1].split("  release-build:\n", 1)[0]
        prepare = script_for(test_job, "Prepare isolated ExifTool source", 6)
        download = script_for(test_job, "Download pinned ExifTool", 6)
        extract = script_for(test_job, "Extract pinned ExifTool into isolated source", 6)
        verify = script_for(test_job, "Install Archive::Zip and verify ExifTool", 6)
        self.assertEqual(test_job.count("path: .ci-cache/exiftool-source.tar.gz"), 2)
        self.assertIn("key: exiftool-src-archive-v1-", test_job)
        self.assertIn("/.ci-cache/", (ROOT / ".gitignore").read_text())
        self.assertIn("if: steps.exiftool-cache.outputs.cache-hit != 'true'", test_job)
        for hit in (False, True):
            with self.subTest(cache_hit=hit), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                archive, fake = self.fixture(root)
                runner_temp = root / "runner-temp"
                runner_temp.mkdir()
                # Simulate persistent old state without touching the host's /tmp.
                old = runner_temp / "oxidex-exiftool-old" / "source"
                old.mkdir(parents=True)
                (old / "stale-from-previous-job").write_text("stale")
                output = root / "output"
                envfile = root / "env"
                pathfile = root / "path"
                env = {**os.environ, "PATH": f"{fake}:{os.environ['PATH']}",
                       "RUNNER_TEMP": str(runner_temp), "GITHUB_OUTPUT": str(output),
                       "GITHUB_WORKSPACE": str(root),
                       "GITHUB_ENV": str(envfile), "GITHUB_PATH": str(pathfile),
                       "FAKE_ARCHIVE": str(archive), "FAKE_CURL_LOG": str(root / "curl-calls"),
                       "V": "13.59"}
                self.run_block(prepare, env)
                source = Path(next(line.removeprefix("source=") for line in
                                   output.read_text().splitlines() if line.startswith("source=")))
                self.assertTrue(source.is_dir())
                self.assertEqual(list(source.iterdir()), [])
                self.assertNotEqual(source, old)
                self.assertEqual((old / "stale-from-previous-job").read_text(), "stale")
                env["EXIFTOOL_SOURCE"] = str(source)
                env["EXIFTOOL_ARCHIVE"] = str(root / ".ci-cache/exiftool-source.tar.gz")
                if hit:
                    Path(env["EXIFTOOL_ARCHIVE"]).parent.mkdir(exist_ok=True)
                    Path(env["EXIFTOOL_ARCHIVE"]).write_bytes(archive.read_bytes())
                else:
                    self.run_block(download, env)
                self.run_block(extract, env)
                calls = root / "curl-calls"
                self.assertEqual(calls.read_text().splitlines() if calls.exists() else [],
                                 [] if hit else ["called"])
                self.assertFalse((source / "stale-from-previous-job").exists())
                self.run_block(verify, env)
                self.assertIn(f"EXIFTOOL={source / 'exiftool'}", envfile.read_text())
                self.assertIn(str(source), pathfile.read_text())

    def test_parallel_cache_misses_publish_complete_archive_for_distinct_sources(self):
        text = CI.read_text()
        test_job = text.split("  test:\n", 1)[1].split("  release-build:\n", 1)[0]
        prepare = script_for(test_job, "Prepare isolated ExifTool source", 6)
        download = script_for(test_job, "Download pinned ExifTool", 6)
        extract = script_for(test_job, "Extract pinned ExifTool into isolated source", 6)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, fake = self.fixture(root)
            runner_temp = root / "runner-temp"
            runner_temp.mkdir()

            def acquire(n: int):
                output, envfile = root / f"output-{n}", root / f"env-{n}"
                env = {**os.environ, "PATH": f"{fake}:{os.environ['PATH']}",
                       "RUNNER_TEMP": str(runner_temp), "GITHUB_WORKSPACE": str(root),
                       "GITHUB_OUTPUT": str(output), "GITHUB_ENV": str(envfile),
                       "FAKE_ARCHIVE": str(archive), "V": "13.59"}
                self.run_block(prepare, env)
                source = Path(next(line.removeprefix("source=") for line in
                                   output.read_text().splitlines() if line.startswith("source=")))
                env.update(EXIFTOOL_SOURCE=str(source),
                           EXIFTOOL_ARCHIVE=str(root / ".ci-cache/exiftool-source.tar.gz"))
                self.run_block(download, env)
                self.run_block(extract, env)
                return source

            with ThreadPoolExecutor(max_workers=2) as pool:
                sources = list(pool.map(acquire, (1, 2)))
            self.assertNotEqual(*sources)
            for source in sources:
                self.assertTrue((source / "exiftool").is_file())
            with tarfile.open(root / ".ci-cache/exiftool-source.tar.gz") as cached:
                self.assertIn("Image-ExifTool-13.59/exiftool", cached.getnames())

    def test_corrupt_cached_archive_fails_before_oracle_export(self):
        text = CI.read_text()
        test_job = text.split("  test:\n", 1)[1].split("  release-build:\n", 1)[0]
        prepare = script_for(test_job, "Prepare isolated ExifTool source", 6)
        extract = script_for(test_job, "Extract pinned ExifTool into isolated source", 6)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runner_temp = root / "runner-temp"
            runner_temp.mkdir()
            output, envfile = root / "output", root / "env"
            env = {**os.environ, "RUNNER_TEMP": str(runner_temp),
                   "GITHUB_OUTPUT": str(output), "GITHUB_ENV": str(envfile),
                   "GITHUB_WORKSPACE": str(root)}
            self.run_block(prepare, env)
            source = Path(next(line.removeprefix("source=") for line in
                               output.read_text().splitlines() if line.startswith("source=")))
            archive = root / ".ci-cache/exiftool-source.tar.gz"
            archive.write_bytes(b"truncated cache archive")
            env.update(EXIFTOOL_SOURCE=str(source), EXIFTOOL_ARCHIVE=str(archive))
            result = subprocess.run(["bash", "-c", extract], env=env,
                                    text=True, capture_output=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(list(source.iterdir()), [])
            self.assertNotIn("EXIFTOOL=", envfile.read_text())

    def test_pinned_action_fetches_into_distinct_clean_source_each_time(self):
        action = ACTION.read_text()
        fetch = script_for(action, "Fetch that ExifTool source", 4)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive, fake = self.fixture(root)
            runner_temp = root / "runner-temp"
            runner_temp.mkdir()
            sources = []
            for attempt in (1, 2):
                envfile = root / f"env-{attempt}"
                env = {**os.environ, "PATH": f"{fake}:{os.environ['PATH']}",
                       "RUNNER_TEMP": str(runner_temp), "GITHUB_ENV": str(envfile),
                       "FAKE_ARCHIVE": str(archive), "ET_VERSION": "13.59"}
                self.run_block(fetch, env)
                source = Path(next(line.removeprefix("EXIFTOOL_SOURCE=") for line in
                                   envfile.read_text().splitlines()
                                   if line.startswith("EXIFTOOL_SOURCE=")))
                sources.append(source)
                self.assertTrue((source / "exiftool").is_file())
                if attempt == 1:
                    (source / "stale-from-previous-job").write_text("stale")
            self.assertNotEqual(*sources)
            self.assertFalse((sources[1] / "stale-from-previous-job").exists())
            self.assertTrue((sources[0] / "stale-from-previous-job").exists())


if __name__ == "__main__":
    unittest.main()
