# ExifTool Coverage Loop — Driver B (Any Model) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two standalone `uv run` Python scripts — `scripts/find_tag_gaps.py` and `scripts/model_fix_loop.py` — that close oxidex/ExifTool tag-coverage gaps in a loop-until-dry cycle, driven by any OpenAI-compatible model API (e.g. GLM-5.2), with no Claude Code dependency.

**Architecture:** `find_tag_gaps.py` wraps `just compare-exiftool-full` / a single-format `tag-comparison` re-run, groups the resulting report's gaps by format, and locates each format's likely parser source. `model_fix_loop.py` consumes that, and for each format sequentially: builds a prompt, calls the model for a unified diff, applies it, builds, retries once on failure, verifies the gap actually shrank and `cargo test --workspace` still passes, then commits or reverts. A round-loop stops after two consecutive rounds close zero gaps.

**Tech Stack:** Python 3.9+ stdlib only (`urllib.request`, `unittest`, `subprocess`, `argparse`, `json`) — no new dependencies, matching this repo's existing `uv run` script convention (see `scripts/jpeg_tag_matrix.py`'s `dependencies = []`). Consumes the existing `src/bin/tag-comparison` Rust binary and `just compare-exiftool-full`.

## Global Constraints

- Local commits only. No push, no PR, ever, from this loop.
- Gap scope is `missing_in_oxidex` + `value_differences` only. Never chase `extra_in_oxidex`.
- A round is "dry" iff it closes zero gaps (not "discovers nothing new"). Two consecutive dry rounds stops the loop.
- Sequential only — no parallelism, no worktree isolation. This is a deliberate simplicity choice for this driver (see spec's Non-goals); do not add concurrency as part of this plan.
- Single-shot patch generation only: one prompt in, one unified diff out, applied mechanically. No agentic tool loop, no shelling out to an external coding-agent CLI.
- On build failure, exactly one repair round-trip (send the compiler/apply error back in the same chat, request a corrected diff) before giving up on that attempt.
- A format that fails twice across rounds moves to a skip-list for the rest of the run, reported in the final summary, never retried again.
- Every commit is gated on both: the targeted re-check showing the gap count decreased, and `cargo test --workspace` passing.
- Python stdlib only — no new PyPI dependencies. Test framework is `unittest` (matches `bindings/python/test_bindings.py`, the only existing Python test in this repo), not `pytest` (not used anywhere in this repo).
- Full design rationale lives in `docs/plans/specs/2026-07-19-exiftool-coverage-loop-driver-b-design.md` — consult it for anything not covered by a task below.

---

## Task 1: Persist `compare-exiftool-full`'s combined-samples directory

Both `find_tag_gaps.py`'s full-corpus discovery and its `--only-format` fast re-check need to point `tag-comparison` at the same combined ExifTool-test-corpus-plus-camera-samples directory across separate script invocations. Today, `just compare-exiftool-full` builds that directory at `/tmp/exiftool-combined-$$` and deletes it via `trap cleanup EXIT` the moment the recipe finishes, making that impossible. (A companion Claude-driven implementation on a separate branch already made this exact fix for its own use — this task applies the same fix independently on this branch, since the two branches don't share history yet.)

**Files:**
- Modify: `justfile:683-793` (the `compare-exiftool-full` recipe only — leave `compare-exiftool-full-update` untouched)

**Interfaces:**
- Produces: a stable path `${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}/combined-samples` and stable exiftool binary path `${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}/exiftool/exiftool`, both persisting across separate `just compare-exiftool-full` invocations. Task 3 (`run_full_comparison`/`run_format_comparison`) references these two paths directly.

- [ ] **Step 1: Change `COMBINED_DIR` to a stable path under the cache dir**

In `justfile`, inside the `compare-exiftool-full` recipe, change:

```sh
    CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}"
    EXIFTOOL_DIR="$CACHE_DIR/exiftool"
    COMBINED_DIR="/tmp/exiftool-combined-$$"
    GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"

    cleanup() {
        echo "🧹 Cleaning up temp files..."
        rm -rf "$COMBINED_DIR"
    }
    trap cleanup EXIT

    mkdir -p "$CACHE_DIR"
```

to:

```sh
    CACHE_DIR="${EXIFTOOL_CACHE_DIR:-/tmp/oxidex-exiftool-cache}"
    EXIFTOOL_DIR="$CACHE_DIR/exiftool"
    # Persistent, not ephemeral: find_tag_gaps.py re-runs tag-comparison
    # directly against this same path from separate script invocations
    # after this recipe has already exited, so it must survive past this
    # shell's lifetime.
    COMBINED_DIR="$CACHE_DIR/combined-samples"
    GCS_BUCKET="https://storage.googleapis.com/oxidex-samples/exiftool"

    mkdir -p "$CACHE_DIR"
```

(This removes the `cleanup()`/`trap` block entirely — nothing else in this recipe needs cleanup on exit.)

- [ ] **Step 2: Verify the recipe still runs and the directory survives**

Run:
```bash
EXIFTOOL_CACHE_DIR=/tmp/oxidex-driverb-test just compare-exiftool-full
ls -d /tmp/oxidex-driverb-test/combined-samples
```
Expected: the recipe completes with `✅ Comprehensive comparison complete!`, and `ls` finds the directory (proving it wasn't deleted on exit).

- [ ] **Step 3: Clean up the test cache dir and commit**

```bash
rm -rf /tmp/oxidex-driverb-test
git add justfile
git commit -m "build: persist compare-exiftool-full's combined-samples dir across runs

find_tag_gaps.py's full-corpus discovery and its --only-format fast
re-check both need to point tag-comparison at the same combined
samples directory from separate script invocations after the recipe
that built it has already exited. Move it from an ephemeral \$\$-tmp
dir (deleted via trap on exit) to a stable path under the existing
cache dir."
```

---

## Task 2: `find_tag_gaps.py` — grouping, ordering, and parser-file location

**Files:**
- Create: `scripts/find_tag_gaps.py`
- Create: `scripts/test_find_tag_gaps.py`
- Create: `tests/fixtures/comparison_report_sample.json`

**Interfaces:**
- Produces: `load_comparison_report(path) -> dict`, `locate_parser_files(format_name, repo_root=REPO_ROOT) -> list[str]`, `group_gaps_by_format(report, repo_root=REPO_ROOT) -> list[dict]` (each dict: `{format, missing_tags, value_differences, gap_count, parser_files}`, sorted by `gap_count` descending, formats with zero gaps excluded). Task 3 and Task 6 both consume `group_gaps_by_format`'s output shape directly.

- [ ] **Step 1: Write the fixture comparison report**

Create `tests/fixtures/comparison_report_sample.json`:

```json
{
  "generated_at": "2026-07-19T00:00:00Z",
  "exiftool_version": "13.55",
  "oxidex_version": "1.2.1",
  "by_format": {
    "PNG": {
      "format": "PNG",
      "files_tested": 3,
      "matched_tags": ["PNG:Width", "PNG:Height"],
      "missing_in_oxidex": [],
      "extra_in_oxidex": [],
      "value_differences": [],
      "regressions": [],
      "coverage_percentage": 100.0,
      "total_exiftool_tags": 2,
      "timestamp": "2026-07-19T00:00:00Z"
    },
    "JPEG": {
      "format": "JPEG",
      "files_tested": 5,
      "matched_tags": ["EXIF:Make"],
      "missing_in_oxidex": [
        {"name": "LensModel", "family": "EXIF", "value": "50mm f/1.8", "tag_id": "0xA434", "source_file": "t/images/Canon.jpg"}
      ],
      "extra_in_oxidex": [],
      "value_differences": [
        {"tag_key": "EXIF:ISO", "exiftool_value": "100", "oxidex_value": "0", "source_file": "t/images/Canon.jpg"}
      ],
      "regressions": [],
      "coverage_percentage": 50.0,
      "total_exiftool_tags": 2,
      "timestamp": "2026-07-19T00:00:00Z"
    },
    "NEF": {
      "format": "NEF",
      "files_tested": 4,
      "matched_tags": [],
      "missing_in_oxidex": [
        {"name": "LensModel", "family": "EXIF", "value": "50mm", "tag_id": null, "source_file": "t/images/Nikon.nef"},
        {"name": "FocusDistance", "family": "MakerNotes", "value": "1.2 m", "tag_id": null, "source_file": "t/images/Nikon.nef"},
        {"name": "ShutterCount", "family": "MakerNotes", "value": "12345", "tag_id": null, "source_file": "t/images/Nikon.nef"}
      ],
      "extra_in_oxidex": [],
      "value_differences": [
        {"tag_key": "EXIF:ISO", "exiftool_value": "400", "oxidex_value": "0", "source_file": "t/images/Nikon.nef"}
      ],
      "regressions": [],
      "coverage_percentage": 0.0,
      "total_exiftool_tags": 4,
      "timestamp": "2026-07-19T00:00:00Z"
    }
  },
  "overall_coverage": 37.5,
  "total_regressions": 0,
  "summary": "Analyzed 3 formats: 3/8 tags (37.5% overall coverage)"
}
```

This mirrors `ComparisonReport`/`FormatComparison`/`TagInfo`/`ValueDifference` from `src/bin/tag-comparison/models/mod.rs` field-for-field (plain Rust `#[derive(Serialize)]`, no `rename_all`, so JSON keys are exactly the snake_case Rust field names). PNG has zero gaps (tests the exclusion filter), JPEG has 2 combined gaps, NEF has 4 (tests descending sort).

- [ ] **Step 2: Write the failing tests for grouping and sorting**

Create `scripts/test_find_tag_gaps.py`:

```python
import json
import unittest
from pathlib import Path

from find_tag_gaps import group_gaps_by_format, locate_parser_files

FIXTURE = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "comparison_report_sample.json"


class GroupGapsByFormatTests(unittest.TestCase):
    def setUp(self):
        with open(FIXTURE) as f:
            self.report = json.load(f)

    def test_sorts_by_gap_count_descending(self):
        gaps = group_gaps_by_format(self.report)
        counts = [g["gap_count"] for g in gaps]
        self.assertEqual(counts, sorted(counts, reverse=True))

    def test_skips_formats_with_no_gaps(self):
        gaps = group_gaps_by_format(self.report)
        formats = {g["format"] for g in gaps}
        self.assertNotIn("PNG", formats)

    def test_gap_count_is_missing_plus_differences(self):
        gaps = group_gaps_by_format(self.report)
        nef = next(g for g in gaps if g["format"] == "NEF")
        self.assertEqual(nef["gap_count"], len(nef["missing_tags"]) + len(nef["value_differences"]))
        self.assertEqual(nef["gap_count"], 4)

    def test_includes_missing_tags_and_value_differences_verbatim(self):
        gaps = group_gaps_by_format(self.report)
        jpeg = next(g for g in gaps if g["format"] == "JPEG")
        self.assertEqual(jpeg["missing_tags"][0]["name"], "LensModel")
        self.assertEqual(jpeg["value_differences"][0]["tag_key"], "EXIF:ISO")


class LocateParserFilesTests(unittest.TestCase):
    def test_jpeg_maps_to_a_real_directory(self):
        files = locate_parser_files("JPEG")
        self.assertTrue(any("src/parsers/jpeg" in f or "src/core" in f for f in files))
        self.assertTrue(len(files) > 0)

    def test_unknown_format_with_no_matching_directory_returns_empty(self):
        files = locate_parser_files("TotallyMadeUpFormat")
        self.assertEqual(files, [])


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd scripts && python3 -m unittest test_find_tag_gaps -v`
Expected: `ModuleNotFoundError: No module named 'find_tag_gaps'` (the module doesn't exist yet).

- [ ] **Step 4: Write the minimal implementation**

Create `scripts/find_tag_gaps.py`:

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Find and group oxidex/ExifTool tag-coverage gaps by format.

Wraps `just compare-exiftool-full` (full corpus) or a direct
`tag-comparison --format` re-run (fast, single-format), then groups the
resulting report's missing_in_oxidex + value_differences by format,
sorted by gap count descending.

Usage:
    uv run scripts/find_tag_gaps.py [--output gaps.json] [--only-format NAME]
                                     [--cache-dir DIR]
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Best-effort format -> source directory/file map, used to hand the model
# real context (it has no file-search tool of its own -- single-shot patch
# generation only). Not authoritative; unlisted formats fall back to a
# lowercase directory guess, and finding nothing is a valid, handled
# outcome (the prompt tells the model these are "likely relevant", not
# exhaustive).
FORMAT_TO_DIR = {
    "JPEG": ["parsers/jpeg", "core"],
    "PNG": ["parsers/png", "core"],
    "TIFF": ["parsers/tiff"],
    "EXIF": ["parsers/tiff"],
    "BMP": ["parsers/image"],
    "GIF": ["parsers/image"],
    "WebP": ["parsers/image"],
    "PDF": ["parsers/pdf"],
    "QuickTime": ["parsers/quicktime"],
    "MP4": ["parsers/quicktime"],
    "MOV": ["parsers/quicktime"],
    "MKV": ["parsers/video"],
    "AVI": ["parsers/video"],
    "RIFF": ["parsers/video"],
    "PE": ["parsers/pe"],
    "ELF": ["parsers/elf"],
    "Mach-O": ["parsers/macho"],
    "ZIP": ["parsers/archive"],
    "DOCX": ["parsers/document"],
    "XLSX": ["parsers/document"],
    "TTF": ["parsers/font"],
    "OTF": ["parsers/font"],
    "DNG": ["parsers/raw"],
    "CR2": ["parsers/raw"],
    "NEF": ["parsers/raw"],
    "ARW": ["parsers/raw"],
    "RAF": ["parsers/raw"],
    "ORF": ["parsers/raw"],
    "RW2": ["parsers/raw"],
    "ICC": ["parsers/icc"],
    "XMP": ["parsers/xmp"],
    "FLAC": ["parsers/audio"],
    "MP3": ["parsers/audio"],
    "AAC": ["parsers/audio"],
    "APE": ["parsers/audio"],
    "Opus": ["parsers/audio"],
    "OGG": ["parsers/audio"],
    "WAV": ["parsers/audio"],
    "FLASHPIX": ["parsers/flashpix"],
    "IPTC": ["parsers/jpeg"],
}


def load_comparison_report(path):
    """Load a tag-comparison ComparisonReport JSON file."""
    with open(path) as f:
        return json.load(f)


def locate_parser_files(format_name, repo_root=REPO_ROOT):
    """Best-effort list of source paths likely responsible for `format_name`.

    Not authoritative -- the model still needs to be told to double-check
    against the actual gap list, but this saves it from starting with
    nothing (it has no file-search tool of its own).
    """
    candidates = FORMAT_TO_DIR.get(format_name, [f"parsers/{format_name.lower()}"])
    found = []
    for rel in candidates:
        path = repo_root / "src" / rel
        if path.is_file():
            found.append(str(path.relative_to(repo_root)))
        elif path.is_dir():
            for rs_file in sorted(path.rglob("*.rs")):
                found.append(str(rs_file.relative_to(repo_root)))
    return found


def group_gaps_by_format(report, repo_root=REPO_ROOT):
    """Group a ComparisonReport's by_format map into a sorted gap list.

    Returns entries only for formats with at least one missing_in_oxidex or
    value_differences entry, sorted by combined gap count descending.
    """
    gaps = []
    for fmt, comp in (report.get("by_format") or {}).items():
        missing = comp.get("missing_in_oxidex") or []
        diffs = comp.get("value_differences") or []
        gap_count = len(missing) + len(diffs)
        if gap_count == 0:
            continue
        gaps.append({
            "format": fmt,
            "missing_tags": missing,
            "value_differences": diffs,
            "gap_count": gap_count,
            "parser_files": locate_parser_files(fmt, repo_root),
        })
    gaps.sort(key=lambda g: g["gap_count"], reverse=True)
    return gaps
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_find_tag_gaps -v`
Expected: all 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git add scripts/find_tag_gaps.py scripts/test_find_tag_gaps.py tests/fixtures/comparison_report_sample.json
git commit -m "feat: add gap grouping/ordering/locator logic to find_tag_gaps.py"
```

---

## Task 3: `find_tag_gaps.py` — CLI, full-corpus run, and fast single-format re-check

**Files:**
- Modify: `scripts/find_tag_gaps.py`
- Modify: `scripts/test_find_tag_gaps.py`

**Interfaces:**
- Consumes: `load_comparison_report`, `group_gaps_by_format` (Task 2).
- Produces: `ensure_tag_comparison_built(repo_root=REPO_ROOT)`, `run_full_comparison(cache_dir, repo_root=REPO_ROOT) -> Path`, `run_format_comparison(format_name, cache_dir, repo_root=REPO_ROOT) -> Path`, a `main(argv=None) -> int` CLI entrypoint. Task 6/7 (`model_fix_loop.py`) import and call `run_full_comparison` and `run_format_comparison` directly.

- [ ] **Step 1: Write the failing tests for the subprocess wrappers**

Append to `scripts/test_find_tag_gaps.py` (add the import at the top alongside the existing one):

```python
from unittest.mock import patch, MagicMock

from find_tag_gaps import run_full_comparison, run_format_comparison
```

```python
class RunFullComparisonTests(unittest.TestCase):
    @patch("find_tag_gaps.subprocess.run")
    def test_invokes_just_with_cache_dir_env(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        result = run_full_comparison("/tmp/fake-cache", repo_root=Path("/fake/repo"))
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], ["just", "compare-exiftool-full"])
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))
        self.assertEqual(kwargs["env"]["EXIFTOOL_CACHE_DIR"], "/tmp/fake-cache")
        self.assertEqual(result, Path("/fake/repo/comparison.json"))


class RunFormatComparisonTests(unittest.TestCase):
    @patch("find_tag_gaps.ensure_tag_comparison_built")
    @patch("find_tag_gaps.subprocess.run")
    def test_invokes_tag_comparison_with_format_flag(self, mock_run, mock_ensure):
        mock_run.return_value = MagicMock(returncode=0)
        result = run_format_comparison("NEF", "/tmp/fake-cache", repo_root=Path("/fake/repo"))
        mock_ensure.assert_called_once_with(Path("/fake/repo"))
        args, kwargs = mock_run.call_args
        self.assertIn("--format", args[0])
        self.assertIn("NEF", args[0])
        self.assertIn("--samples", args[0])
        self.assertIn("/tmp/fake-cache/combined-samples", args[0])
        self.assertEqual(result, Path("/tmp/tagcmp-NEF.json"))
```

- [ ] **Step 2: Run to verify these fail**

Run: `cd scripts && python3 -m unittest test_find_tag_gaps -v`
Expected: `ImportError: cannot import name 'run_full_comparison'` (not yet defined).

- [ ] **Step 3: Implement the subprocess wrappers and CLI**

Append to `scripts/find_tag_gaps.py` (after `group_gaps_by_format`):

```python
def ensure_tag_comparison_built(repo_root=REPO_ROOT):
    subprocess.run(
        ["cargo", "build", "--release", "--bin", "tag-comparison", "--features", "tag-comparison-binary"],
        cwd=repo_root, check=True,
    )


def run_full_comparison(cache_dir, repo_root=REPO_ROOT):
    """Run `just compare-exiftool-full` and return the path to comparison.json."""
    subprocess.run(
        ["just", "compare-exiftool-full"],
        cwd=repo_root,
        env={**os.environ, "EXIFTOOL_CACHE_DIR": str(cache_dir)},
        check=True,
    )
    return repo_root / "comparison.json"


def run_format_comparison(format_name, cache_dir, repo_root=REPO_ROOT):
    """Re-run tag-comparison for a single format against the cached samples.

    Requires run_full_comparison to have populated cache_dir at least once
    (this does not download or build the combined samples itself).
    """
    ensure_tag_comparison_built(repo_root)
    output = Path(f"/tmp/tagcmp-{format_name}.json")
    subprocess.run(
        [
            str(repo_root / "target/release/tag-comparison"),
            "--exiftool", f"{cache_dir}/exiftool/exiftool",
            "--samples", f"{cache_dir}/combined-samples",
            "--format", format_name,
            "-o", str(output),
            "--markdown-dir", f"/tmp/tagcmp-{format_name}-md",
        ],
        cwd=repo_root, check=True,
    )
    return output


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="gaps.json")
    parser.add_argument("--only-format")
    parser.add_argument("--cache-dir", default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))
    args = parser.parse_args(argv)

    if args.only_format:
        report_path = run_format_comparison(args.only_format, args.cache_dir)
    else:
        report_path = run_full_comparison(args.cache_dir)

    report = load_comparison_report(report_path)
    gaps = group_gaps_by_format(report)
    if args.only_format:
        gaps = [g for g in gaps if g["format"] == args.only_format]

    with open(args.output, "w") as f:
        json.dump(gaps, f, indent=2)

    total = sum(g["gap_count"] for g in gaps)
    print(f"{len(gaps)} formats with gaps, {total} total gaps -> {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_find_tag_gaps -v`
Expected: all 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add scripts/find_tag_gaps.py scripts/test_find_tag_gaps.py
git commit -m "feat: add CLI and compare-exiftool subprocess wrappers to find_tag_gaps.py"
```

---

## Task 4: `model_fix_loop.py` — HTTP client and diff extraction

**Files:**
- Create: `scripts/model_fix_loop.py`
- Create: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: `extract_diff(response_text) -> str | None`, `call_model(messages, base_url, api_key, model) -> str`. Task 6 (`fix_gap`) consumes both by name.

- [ ] **Step 1: Write the failing tests**

Create `scripts/test_model_fix_loop.py`:

```python
import json
import unittest
from unittest.mock import patch, MagicMock

from model_fix_loop import extract_diff, call_model


class ExtractDiffTests(unittest.TestCase):
    def test_extracts_fenced_diff_block(self):
        text = (
            "Here is the fix:\n```diff\n--- a/foo.rs\n+++ b/foo.rs\n"
            "@@ -1 +1 @@\n-old\n+new\n```\nDone."
        )
        diff = extract_diff(text)
        self.assertTrue(diff.startswith("--- a/foo.rs"))
        self.assertIn("+new", diff)

    def test_falls_back_to_bare_diff_git_header(self):
        text = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1 +1 @@\n-old\n+new\n"
        self.assertEqual(extract_diff(text), text)

    def test_returns_none_when_no_diff_present(self):
        self.assertIsNone(extract_diff("I don't know how to fix this."))


class CallModelTests(unittest.TestCase):
    @patch("model_fix_loop.urllib.request.urlopen")
    def test_posts_expected_body_and_parses_reply(self, mock_urlopen):
        response_json = json.dumps({"choices": [{"message": {"content": "the diff"}}]}).encode()
        mock_cm = MagicMock()
        mock_cm.read.return_value = response_json
        mock_urlopen.return_value.__enter__.return_value = mock_cm

        result = call_model(
            [{"role": "user", "content": "fix it"}],
            base_url="https://api.z.ai/api/paas/v4",
            api_key="secret",
            model="glm-5.2",
        )

        self.assertEqual(result, "the diff")
        request = mock_urlopen.call_args[0][0]
        self.assertEqual(request.full_url, "https://api.z.ai/api/paas/v4/chat/completions")
        self.assertEqual(request.get_header("Authorization"), "Bearer secret")
        body = json.loads(request.data)
        self.assertEqual(body["model"], "glm-5.2")
        self.assertEqual(body["messages"], [{"role": "user", "content": "fix it"}])


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd scripts && python3 -m unittest test_model_fix_loop -v`
Expected: `ModuleNotFoundError: No module named 'model_fix_loop'`.

- [ ] **Step 3: Implement**

Create `scripts/model_fix_loop.py`:

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Close oxidex/ExifTool tag-coverage gaps via any OpenAI-compatible model API.

Config (env vars, or matching --flags):
    MODEL_FIX_BASE_URL   e.g. https://api.z.ai/api/paas/v4  (GLM-5.2)
    MODEL_FIX_API_KEY
    MODEL_FIX_MODEL       e.g. "glm-5.2"

Usage:
    uv run scripts/model_fix_loop.py
"""
import argparse
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

from find_tag_gaps import (
    REPO_ROOT,
    group_gaps_by_format,
    load_comparison_report,
    run_format_comparison,
    run_full_comparison,
)

DIFF_BLOCK_RE = re.compile(r"```diff\n(.*?)```", re.DOTALL)


def extract_diff(response_text):
    """Pull a unified diff out of a chat response.

    Prefers a fenced ```diff block; falls back to treating the whole
    response as a diff if it looks like one (starts with "diff --git" or
    "--- "). Returns None if nothing diff-shaped is found.
    """
    match = DIFF_BLOCK_RE.search(response_text)
    if match:
        return match.group(1).strip() + "\n"
    stripped = response_text.strip()
    if stripped.startswith("diff --git") or stripped.startswith("--- "):
        return stripped + "\n"
    return None


def call_model(messages, base_url, api_key, model):
    """POST a chat-completions request, return the assistant's reply text."""
    url = base_url.rstrip("/") + "/chat/completions"
    body = json.dumps({"model": model, "messages": messages, "temperature": 0}).encode()
    req = urllib.request.Request(
        url, data=body, method="POST",
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        payload = json.loads(resp.read())
    return payload["choices"][0]["message"]["content"]
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_model_fix_loop -v`
Expected: all 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: add model HTTP client and diff extraction to model_fix_loop.py"
```

---

## Task 5: `model_fix_loop.py` — git and cargo helpers

**Files:**
- Modify: `scripts/model_fix_loop.py`
- Modify: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: `git_apply(diff_text, repo_root) -> (bool, str)`, `git_checkout_clean(repo_root) -> None`, `git_commit(message, repo_root) -> None`, `cargo_build(repo_root) -> (bool, str)`, `cargo_test_workspace(repo_root) -> bool`. Task 6 (`fix_gap`) takes each as an injectable dependency with these exact signatures.

- [ ] **Step 1: Write the failing tests**

Append to `scripts/test_model_fix_loop.py` (add to the existing import line):

```python
from model_fix_loop import (
    cargo_build,
    cargo_test_workspace,
    call_model,
    extract_diff,
    git_apply,
    git_checkout_clean,
    git_commit,
)
from pathlib import Path
```

```python
class GitApplyTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_success_returns_true(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stderr="")
        ok, msg = git_apply("diff text", Path("/fake/repo"))
        self.assertTrue(ok)
        args, kwargs = mock_run.call_args
        self.assertEqual(args[0], ["git", "apply", "--reject", "-"])
        self.assertEqual(kwargs["input"], "diff text")
        self.assertEqual(kwargs["cwd"], Path("/fake/repo"))

    @patch("model_fix_loop.subprocess.run")
    def test_failure_returns_stderr(self, mock_run):
        mock_run.return_value = MagicMock(returncode=1, stderr="patch does not apply")
        ok, msg = git_apply("bad diff", Path("/fake/repo"))
        self.assertFalse(ok)
        self.assertEqual(msg, "patch does not apply")


class GitCheckoutCleanTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_runs_checkout_then_clean(self, mock_run):
        git_checkout_clean(Path("/fake/repo"))
        calls = [c.args[0] for c in mock_run.call_args_list]
        self.assertIn(["git", "checkout", "--", "."], calls)
        self.assertIn(["git", "clean", "-fd"], calls)


class GitCommitTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_adds_then_commits_with_message(self, mock_run):
        git_commit("fix(nef): wire tags", Path("/fake/repo"))
        calls = [c.args[0] for c in mock_run.call_args_list]
        self.assertIn(["git", "add", "-A"], calls)
        self.assertIn(["git", "commit", "-m", "fix(nef): wire tags"], calls)


class CargoBuildTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_reports_failure_with_stderr(self, mock_run):
        mock_run.return_value = MagicMock(returncode=101, stderr="error[E0308]: mismatched types")
        ok, err = cargo_build(Path("/fake/repo"))
        self.assertFalse(ok)
        self.assertIn("E0308", err)

    @patch("model_fix_loop.subprocess.run")
    def test_reports_success(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stderr="")
        ok, err = cargo_build(Path("/fake/repo"))
        self.assertTrue(ok)


class CargoTestWorkspaceTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_true_on_zero_exit(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0)
        self.assertTrue(cargo_test_workspace(Path("/fake/repo")))

    @patch("model_fix_loop.subprocess.run")
    def test_false_on_nonzero_exit(self, mock_run):
        mock_run.return_value = MagicMock(returncode=1)
        self.assertFalse(cargo_test_workspace(Path("/fake/repo")))
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd scripts && python3 -m unittest test_model_fix_loop -v`
Expected: `ImportError: cannot import name 'git_apply'`.

- [ ] **Step 3: Implement**

Append to `scripts/model_fix_loop.py` (after `call_model`):

```python
def git_apply(diff_text, repo_root):
    """Apply a unified diff to the working tree. Returns (success, message)."""
    result = subprocess.run(
        ["git", "apply", "--reject", "-"],
        input=diff_text, capture_output=True, text=True, cwd=repo_root,
    )
    if result.returncode == 0:
        return True, "applied"
    return False, result.stderr


def git_checkout_clean(repo_root):
    """Discard all uncommitted changes, including untracked files."""
    subprocess.run(["git", "checkout", "--", "."], cwd=repo_root, check=True)
    subprocess.run(["git", "clean", "-fd"], cwd=repo_root, check=True)


def git_commit(message, repo_root):
    subprocess.run(["git", "add", "-A"], cwd=repo_root, check=True)
    subprocess.run(["git", "commit", "-m", message], cwd=repo_root, check=True)


def cargo_build(repo_root):
    """Build the oxidex binary. Returns (success, stderr)."""
    result = subprocess.run(
        ["cargo", "build", "--release", "--bin", "oxidex"],
        capture_output=True, text=True, cwd=repo_root,
    )
    return result.returncode == 0, result.stderr


def cargo_test_workspace(repo_root):
    """Run the full workspace test suite. Returns True if all tests pass."""
    result = subprocess.run(
        ["cargo", "test", "--workspace"],
        capture_output=True, text=True, cwd=repo_root,
    )
    return result.returncode == 0
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_model_fix_loop -v`
Expected: all 12 tests pass.

- [ ] **Step 5: Commit**

```bash
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: add git/cargo subprocess helpers to model_fix_loop.py"
```

---

## Task 6: `model_fix_loop.py` — the `fix_gap` state machine

**Files:**
- Modify: `scripts/model_fix_loop.py`
- Modify: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Consumes: `extract_diff`, `call_model`, `git_apply`, `git_checkout_clean`, `git_commit`, `cargo_build`, `cargo_test_workspace` (Tasks 4-5), all as injectable keyword-arg defaults.
- Produces: `build_prompt(gap) -> str`, `fix_gap(gap, config, *, call_model_fn=call_model, git_apply_fn=git_apply, git_checkout_clean_fn=git_checkout_clean, git_commit_fn=git_commit, cargo_build_fn=cargo_build, cargo_test_workspace_fn=cargo_test_workspace, recheck_fn=None, repo_root=None) -> dict` (`{"format": str, "status": "fixed"|"failed", "gaps_closed": int}` on success, `{"format": str, "status": "failed", "reason": str}` on failure). Task 7 (`run_loop`) calls `fix_gap` once per gap per round.

- [ ] **Step 1: Write the failing tests**

Append to `scripts/test_model_fix_loop.py` (add to the import line):

```python
from model_fix_loop import fix_gap
```

```python
def make_gap(gap_count=2):
    return {
        "format": "NEF",
        "missing_tags": [
            {"family": "EXIF", "name": "LensModel", "value": "50mm", "tag_id": None, "source_file": "a.nef"}
        ],
        "value_differences": [
            {"tag_key": "EXIF:ISO", "exiftool_value": "100", "oxidex_value": "0", "source_file": "a.nef"}
        ],
        "gap_count": gap_count,
        "parser_files": [],
    }


class FixGapHappyPathTests(unittest.TestCase):
    def test_commits_when_build_and_tests_pass_and_gaps_shrink(self):
        gap = make_gap(gap_count=2)
        model_calls = []
        commit_calls = []

        result = fix_gap(
            gap,
            {"base_url": "u", "api_key": "k", "model": "glm-5.2"},
            call_model_fn=lambda messages, *a: (model_calls.append(1), "```diff\n--- a/x\n+++ b/x\n```\n")[1],
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: commit_calls.append(msg),
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(result["gaps_closed"], 2)
        self.assertEqual(len(model_calls), 1)
        self.assertEqual(len(commit_calls), 1)
        self.assertIn("glm-5.2", commit_calls[0])


class FixGapRepairRoundTripTests(unittest.TestCase):
    def test_retries_once_on_build_failure_then_succeeds(self):
        gap = make_gap(gap_count=1)
        build_attempts = []

        def fake_cargo_build(root):
            build_attempts.append(1)
            if len(build_attempts) == 1:
                return False, "error[E0308]: mismatched types"
            return True, ""

        result = fix_gap(
            gap,
            {"base_url": "u", "api_key": "k", "model": "glm-5.2"},
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: None,
            cargo_build_fn=fake_cargo_build,
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(len(build_attempts), 2)


class FixGapFailureTests(unittest.TestCase):
    def test_fails_after_two_build_failures(self):
        gap = make_gap()
        result = fix_gap(
            gap,
            {"base_url": "u", "api_key": "k", "model": "glm-5.2"},
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            cargo_build_fn=lambda root: (False, "still broken"),
            cargo_test_workspace_fn=lambda root: True,
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(result["status"], "failed")

    def test_fails_when_gap_count_does_not_decrease(self):
        gap = make_gap(gap_count=2)
        result = fix_gap(
            gap,
            {"base_url": "u", "api_key": "k", "model": "glm-5.2"},
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 2,
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["reason"], "gap count did not decrease")

    def test_fails_when_tests_regress(self):
        gap = make_gap(gap_count=2)
        result = fix_gap(
            gap,
            {"base_url": "u", "api_key": "k", "model": "glm-5.2"},
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            cargo_build_fn=lambda root: (True, ""),
            cargo_test_workspace_fn=lambda root: False,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["reason"], "cargo test --workspace regressed")

    def test_fails_when_no_diff_in_response(self):
        gap = make_gap()
        result = fix_gap(
            gap,
            {"base_url": "u", "api_key": "k", "model": "glm-5.2"},
            call_model_fn=lambda messages, *a: "I could not find a fix.",
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            repo_root=Path("/fake/repo"),
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["reason"], "no diff in model response")
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd scripts && python3 -m unittest test_model_fix_loop -v`
Expected: `ImportError: cannot import name 'fix_gap'`.

- [ ] **Step 3: Implement**

Append to `scripts/model_fix_loop.py` (after `cargo_test_workspace`):

```python
def build_prompt(gap):
    missing = "\n".join(
        f"  - {t['family']}:{t['name']} = {t['value']} (sample: {t.get('source_file') or 'n/a'})"
        for t in gap["missing_tags"]
    ) or "  (none)"
    diffs = "\n".join(
        f"  - {d['tag_key']}: exiftool=\"{d['exiftool_value']}\" oxidex=\"{d['oxidex_value']}\" (sample: {d['source_file']})"
        for d in gap["value_differences"]
    ) or "  (none)"
    file_blocks = []
    for f in gap["parser_files"]:
        try:
            file_blocks.append(f"--- {f} ---\n{Path(f).read_text()}")
        except OSError:
            continue
    files = "\n\n".join(file_blocks) or "(no parser files located -- search src/ yourself)"
    return (
        f"You are fixing ExifTool tag-coverage gaps in the oxidex Rust codebase, format \"{gap['format']}\".\n\n"
        f"Missing entirely (ExifTool extracts it, oxidex doesn't):\n{missing}\n\n"
        f"Value differences (both extract it, values disagree):\n{diffs}\n\n"
        f"Likely relevant source files:\n{files}\n\n"
        "Respond with a single unified diff (in a ```diff fenced block) that fixes as many of these gaps "
        "as you can correctly verify. For value differences, only fix genuine bugs, not benign formatting "
        "differences. Do not include any explanation outside the diff."
    )


def fix_gap(gap, config, *, call_model_fn=call_model, git_apply_fn=git_apply,
            git_checkout_clean_fn=git_checkout_clean, git_commit_fn=git_commit,
            cargo_build_fn=cargo_build, cargo_test_workspace_fn=cargo_test_workspace,
            recheck_fn=None, repo_root=None):
    """Attempt to close one format's gaps via a single-shot patch, with one
    repair round-trip on build failure. Returns a result dict.

    recheck_fn(format_name) -> int must return the gap count for that
    format after the attempted fix (used to confirm real progress). If not
    provided, progress can never be confirmed and the attempt always fails
    the "gap count did not decrease" check.
    """
    repo_root = repo_root or REPO_ROOT
    messages = [{"role": "user", "content": build_prompt(gap)}]

    built = False
    for _attempt in range(2):  # one initial attempt + one repair round-trip
        reply = call_model_fn(messages, config["base_url"], config["api_key"], config["model"])
        diff = extract_diff(reply)
        if diff is None:
            return {"format": gap["format"], "status": "failed", "reason": "no diff in model response"}

        messages.append({"role": "assistant", "content": reply})

        applied, apply_msg = git_apply_fn(diff, repo_root)
        if not applied:
            git_checkout_clean_fn(repo_root)
            messages.append({
                "role": "user",
                "content": f"That diff did not apply: {apply_msg}\nPlease resend a corrected diff.",
            })
            continue

        built, build_err = cargo_build_fn(repo_root)
        if built:
            break

        git_checkout_clean_fn(repo_root)
        messages.append({
            "role": "user",
            "content": f"The build failed:\n{build_err}\nPlease resend a corrected diff.",
        })

    if not built:
        return {"format": gap["format"], "status": "failed", "reason": "no working fix after repair attempt"}

    remaining = recheck_fn(gap["format"]) if recheck_fn else gap["gap_count"]
    if remaining >= gap["gap_count"]:
        git_checkout_clean_fn(repo_root)
        return {"format": gap["format"], "status": "failed", "reason": "gap count did not decrease"}

    if not cargo_test_workspace_fn(repo_root):
        git_checkout_clean_fn(repo_root)
        return {"format": gap["format"], "status": "failed", "reason": "cargo test --workspace regressed"}

    closed = gap["gap_count"] - remaining
    git_commit_fn(f"fix({gap['format'].lower()}): wire {closed} missing tags (via {config['model']})", repo_root)
    return {"format": gap["format"], "status": "fixed", "gaps_closed": closed}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_model_fix_loop -v`
Expected: all 18 tests pass.

- [ ] **Step 5: Commit**

```bash
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: add fix_gap single-shot-patch state machine to model_fix_loop.py"
```

---

## Task 7: `model_fix_loop.py` — round loop, skip-list, and CLI entrypoint

**Files:**
- Modify: `scripts/model_fix_loop.py`
- Modify: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Consumes: `fix_gap` (Task 6), `run_full_comparison`, `run_format_comparison`, `group_gaps_by_format`, `load_comparison_report` (Task 3, already imported).
- Produces: `run_loop(config, find_gaps_fn, fix_gap_fn, max_dry_rounds=2) -> dict` (`{"rounds": int, "fixed": list, "failed": list, "skipped": list}`), a `main(argv=None) -> int` CLI entrypoint. Nothing downstream depends on this file further -- this is the last task.

- [ ] **Step 1: Write the failing tests**

Append to `scripts/test_model_fix_loop.py` (add to the import line):

```python
from model_fix_loop import run_loop
```

```python
class RunLoopTests(unittest.TestCase):
    def test_stops_after_two_consecutive_dry_rounds(self):
        find_calls = []

        def fake_find_gaps():
            find_calls.append(1)
            return []

        result = run_loop({"model": "x"}, fake_find_gaps, fix_gap_fn=lambda g, c: self.fail("should not fix"))
        self.assertEqual(result["rounds"], 2)
        self.assertEqual(len(find_calls), 2)

    def test_resets_dry_streak_when_a_gap_closes(self):
        rounds = [[make_gap()], [], []]

        def fake_find_gaps():
            return rounds.pop(0)

        def fake_fix_gap(gap, config):
            return {"format": gap["format"], "status": "fixed", "gaps_closed": gap["gap_count"]}

        result = run_loop({"model": "x"}, fake_find_gaps, fake_fix_gap)
        self.assertEqual(result["rounds"], 3)
        self.assertEqual(len(result["fixed"]), 1)

    def test_skips_a_format_that_fails_twice(self):
        gap = make_gap()
        attempts = []
        rounds = [[gap], [gap]]

        def fake_find_gaps():
            return rounds.pop(0) if rounds else []

        def fake_fix_gap(g, config):
            attempts.append(g["format"])
            return {"format": g["format"], "status": "failed", "reason": "still broken"}

        result = run_loop({"model": "x"}, fake_find_gaps, fake_fix_gap)
        self.assertEqual(attempts, ["NEF", "NEF"])
        self.assertEqual(result["skipped"], ["NEF"])
        self.assertEqual(result["rounds"], 2)
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd scripts && python3 -m unittest test_model_fix_loop -v`
Expected: `ImportError: cannot import name 'run_loop'`.

- [ ] **Step 3: Implement**

Append to `scripts/model_fix_loop.py` (after `fix_gap`):

```python
def run_loop(config, find_gaps_fn, fix_gap_fn, max_dry_rounds=2):
    """Loop-until-dry driver. Returns a summary dict.

    A round is dry iff it closes zero gaps (not "discovers nothing new").
    A format that fails twice across rounds is skipped for the rest of
    the run.
    """
    skip_list = set()
    fail_counts = {}
    fixed, failed, skipped = [], [], []
    dry_rounds = 0
    round_num = 0

    while dry_rounds < max_dry_rounds:
        round_num += 1
        gaps = [g for g in find_gaps_fn() if g["format"] not in skip_list]
        if not gaps:
            dry_rounds += 1
            continue

        closed_this_round = 0
        for gap in gaps:
            result = fix_gap_fn(gap, config)
            if result["status"] == "fixed":
                fixed.append(result)
                closed_this_round += 1
            else:
                fail_counts[gap["format"]] = fail_counts.get(gap["format"], 0) + 1
                if fail_counts[gap["format"]] >= 2:
                    skip_list.add(gap["format"])
                    skipped.append(gap["format"])
                else:
                    failed.append(result)

        dry_rounds = 0 if closed_this_round else dry_rounds + 1

    return {
        "rounds": round_num,
        "fixed": fixed,
        "failed": failed,
        "skipped": sorted(set(skipped)),
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default=os.environ.get("MODEL_FIX_BASE_URL"))
    parser.add_argument("--api-key", default=os.environ.get("MODEL_FIX_API_KEY"))
    parser.add_argument("--model", default=os.environ.get("MODEL_FIX_MODEL"))
    parser.add_argument("--cache-dir", default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))
    args = parser.parse_args(argv)

    if not (args.base_url and args.api_key and args.model):
        print(
            "MODEL_FIX_BASE_URL, MODEL_FIX_API_KEY, and MODEL_FIX_MODEL "
            "(or --base-url/--api-key/--model) are all required",
            file=sys.stderr,
        )
        return 1

    config = {"base_url": args.base_url, "api_key": args.api_key, "model": args.model}

    def find_gaps_fn():
        report_path = run_full_comparison(args.cache_dir)
        return group_gaps_by_format(load_comparison_report(report_path))

    def real_fix_gap(gap, cfg):
        def recheck(fmt):
            path = run_format_comparison(fmt, args.cache_dir)
            regrouped = group_gaps_by_format(load_comparison_report(path))
            match = next((g for g in regrouped if g["format"] == fmt), None)
            return match["gap_count"] if match else 0

        return fix_gap(gap, cfg, recheck_fn=recheck)

    summary = run_loop(config, find_gaps_fn, real_fix_gap)
    print(f"stopped after {summary['rounds']} rounds")
    print(f"  fixed:   {len(summary['fixed'])} formats")
    print(f"  failed:  {len(summary['failed'])} attempts")
    print(f"  skipped: {', '.join(summary['skipped']) or '(none)'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_model_fix_loop -v`
Expected: all 21 tests pass.

- [ ] **Step 5: Commit**

```bash
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: add round loop, skip-list, and CLI entrypoint to model_fix_loop.py"
```

---

## Task 8: Manual end-to-end validation

This requires a real ExifTool binary, network access, and a real model API key -- it cannot be automated as part of this plan's test suite. Do this once Tasks 1-7 are committed.

- [ ] **Step 1: Confirm both scripts run standalone with `uv run`**

```bash
uv run scripts/find_tag_gaps.py --only-format JPEG --cache-dir /tmp/oxidex-exiftool-cache
```

Expected: builds `tag-comparison` if needed, runs it against JPEG samples (requires `/tmp/oxidex-exiftool-cache/combined-samples` to already exist from a prior full run -- if it doesn't, run `just compare-exiftool-full` once first), and writes `gaps.json` with a summary line like `1 formats with gaps, N total gaps -> gaps.json`.

- [ ] **Step 2: Run one real round against a small, known-small format**

Pick a format with a small gap count from Step 1's output (or run without `--only-format` once and inspect `gaps.json` for the smallest nonzero entry). Set real credentials and run:

```bash
export MODEL_FIX_BASE_URL=https://api.z.ai/api/paas/v4
export MODEL_FIX_API_KEY=<your key>
export MODEL_FIX_MODEL=glm-5.2
uv run scripts/model_fix_loop.py
```

Expected: real, possibly multi-minute run (network calls to the model API, `cargo build --release`, `cargo test --workspace`). Confirm via `git log --oneline -5` afterward that any `fixed` gap produced exactly one commit with a message matching `fix(<format>): wire N missing tags (via glm-5.2)`, and that `cargo test --workspace` passes on `HEAD`. If nothing closed in the first round, that's an acceptable real-world outcome (the model may fail to produce a working diff) -- confirm instead that the working tree is clean (`git status --short` empty) and the printed summary's `failed`/`skipped` counts match what happened.

- [ ] **Step 3: Confirm the stop condition**

Let it run to completion (or interrupt after confirming at least one full loop-until-dry cycle if the gap backlog is large). Expected: the final printed summary's `rounds` count reflects two consecutive dry rounds having occurred, and the process exits with code 0.

---

## Notes for whoever executes this plan

- Each `fix_gap` call in real use is a real, possibly multi-minute operation (a model API round-trip, a full `cargo build --release`, and a full `cargo test --workspace`). Don't expect unit-test-speed iteration when running the actual loop; the automated tests in Tasks 2-7 use injected fakes specifically so the TDD cycle itself stays fast.
- This plan's scripts (`find_tag_gaps.py`, `run_full_comparison`) share the same `EXIFTOOL_CACHE_DIR` convention as the companion Claude-driven implementation on `claude/exiftool-coverage-loop-96b462`, so a warm cache built by one can be reused by the other if run on the same machine -- but the two implementations are otherwise fully independent (different branches, different worktrees, no shared code).
- Full rationale for every decision here (why sequential-only, why single-shot patch generation over an agentic tool loop, why the trust-boundary note about sending source to a third-party endpoint) is in `docs/plans/specs/2026-07-19-exiftool-coverage-loop-driver-b-design.md`. If a step in this plan seems to contradict that document, the spec wins -- flag it rather than silently picking one.
