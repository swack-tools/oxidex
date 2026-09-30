"""Exercise the indicative benchmark workflow's runner attribution without timing."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/benchmarks.yml").read_text()
SCRIPT = (ROOT / "benches/exiftool_comparison.sh").read_text()


class BenchmarkAttributionTests(unittest.TestCase):
    def test_workflow_environment_stamps_actual_spot_runner(self):
        assignment = next(
            line.strip()
            for line in WORKFLOW.splitlines()
            if line.strip().startswith("BENCH_ENVIRONMENT=")
        )
        with tempfile.TemporaryDirectory() as directory:
            bin_dir = Path(directory)
            nproc = bin_dir / "nproc"
            nproc.write_text("#!/bin/sh\necho 8\n")
            nproc.chmod(0o755)
            env = os.environ.copy()
            env.update(
                PATH=f"{bin_dir}:{env['PATH']}",
                CPU="Test CPU",
                RUNNER_NAME="spot-node-42",
                RUNNER_OS="Linux",
                RUNNER_ARCH="X64",
                RUNNER_LABEL="ubuntu-24.04",
                ImageVersion="hosted-image-42",
                GITHUB_SERVER_URL="https://github.com",
                GITHUB_REPOSITORY="example/oxidex",
                GITHUB_RUN_ID="12345",
                GITHUB_RUN_ATTEMPT="2",
            )
            result = subprocess.run(
                ["bash", "-c", assignment + '\nprintf "%s" "$BENCH_ENVIRONMENT"'],
                env=env,
                capture_output=True,
                text=True,
                check=True,
            )
        value = result.stdout
        self.assertIn("INDICATIVE ONLY", value)
        self.assertIn("self-hosted spot pool; actual runner spot-node-42", value)
        self.assertIn("Linux/X64, 8 vCPU, Test CPU", value)
        self.assertIn("/actions/runs/12345 attempt 2", value)
        self.assertNotIn("ubuntu-24.04", value)
        self.assertNotIn("hosted-image-42", value)
        # The measured report's Markdown and JSON both consume this value.
        self.assertIn("**Environment**: ${BENCH_ENVIRONMENT:-", SCRIPT)
        self.assertIn('--arg env "${BENCH_ENVIRONMENT:-', SCRIPT)

    def test_step_summary_identifies_runner_on_success_and_refusal(self):
        section = WORKFLOW.split("      - name: Write step summary", 1)[1]
        body = section.split("          python3 - <<'PY'\n", 1)[1].split(
            "\n          PY", 1
        )[0]
        program = textwrap.dedent(body)
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            bin_dir = work / "bin"
            bin_dir.mkdir()
            for name, output in (("nproc", "8"), ("lscpu", "Model name: Test CPU")):
                command = bin_dir / name
                command.write_text(f"#!/bin/sh\necho '{output}'\n")
                command.chmod(0o755)
            (work / "benches").mkdir()
            subprocess.run(["git", "init", "-q", str(work)], check=True)
            subprocess.run(
                ["git", "-C", str(work), "-c", "user.name=Test", "-c",
                 "user.email=test@example.com", "commit", "-q", "--allow-empty",
                 "-m", "fixture"],
                check=True,
            )
            commit = subprocess.check_output(
                ["git", "-C", str(work), "rev-parse", "HEAD"], text=True
            ).strip()
            scenario = {"results": [{"median": 0.02, "min": 0.01},
                                    {"median": 0.01, "min": 0.005}]}
            report = {
                "instrument": {
                    "commit": commit,
                    "exiftool": "pinned ExifTool",
                    "oxidex_version": "oxidex test",
                    "oxidex_sha256": "a" * 64,
                    "staleness_note": "",
                    "machine": "Linux x86_64",
                    "loadavg_at_preflight": "1 1 1",
                },
                **{key: scenario for key in (
                    "single_file", "single_canon", "batch", "write",
                    "detection", "corpus", "corpus_1thread")},
            }
            (work / "benches/benchmark_results.json").write_text(json.dumps(report))
            for outcome in ("success", "failure"):
                with self.subTest(outcome=outcome):
                    summary = work / "summary.md"
                    summary.write_text("")
                    env = os.environ.copy()
                    env.update(
                        PATH=f"{bin_dir}:{env['PATH']}",
                        GITHUB_STEP_SUMMARY=str(summary), EVENT="push",
                        BENCH_OUTCOME=outcome, RUNNER_NAME="spot-node-42",
                        RUNNER_OS="Linux", RUNNER_ARCH="X64",
                        RUNNER_LABEL="ubuntu-24.04", ImageVersion="hosted-image-42",
                        HYPERFINE_WARMUP="3", HYPERFINE_RUNS="20",
                    )
                    subprocess.run(
                        ["python3", "-c", program], cwd=work, env=env,
                        capture_output=True, text=True, check=True,
                    )
                    rendered = summary.read_text()
                    self.assertIn("Self-hosted spot runner", rendered)
                    self.assertIn("indicative only", rendered)
                    self.assertIn("no cross-run", rendered)
                    self.assertNotIn("GitHub-hosted", rendered)
                    self.assertNotIn("ubuntu-24.04", rendered)
                    self.assertNotIn("hosted-image-42", rendered)
                    if outcome == "success":
                        self.assertIn("actual runner spot-node-42", rendered)
                        self.assertIn("Linux/X64", rendered)
                        self.assertIn("8 vCPU, Test CPU", rendered)
                        self.assertIn("Runner vCPU count: 8", rendered)
                    else:
                        self.assertIn("No results", rendered)
                        self.assertNotIn("actual runner spot-node-42", rendered)


if __name__ == "__main__":
    unittest.main()
