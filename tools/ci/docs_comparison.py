#!/usr/bin/env python3
"""Generate hosted-CI docs using the pinned source and locked sample archives.

This report is not a maintainer release-qualification receipt. It uses the
runner's capability-checked Perl, not the durable release installation.
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.release import bootstrap_oracle as bootstrap


def generate(repo: Path) -> None:
    source_value = os.environ.get("EXIFTOOL_SOURCE")
    if not source_value:
        raise ValueError("EXIFTOOL_SOURCE is required; run the pinned-exiftool CI action first")
    source = Path(source_value).resolve()
    cache = Path(os.environ["EXIFTOOL_CACHE_DIR"]).resolve()
    if cache.is_relative_to(bootstrap.DURABLE_ROOT):
        raise ValueError("Hosted docs cache must be separate from the durable release root")
    pin = (repo / ".exiftool-version").read_text().strip()
    if pin != bootstrap.LOCK["exiftool"]["version"]:
        raise ValueError("Docs corpus lock does not match .exiftool-version")
    perl = os.environ.get("OXIDEX_TABLES_PERL") or shutil.which("perl")
    if not perl:
        raise ValueError("Perl is unavailable")
    environment = {**os.environ, "EXIFTOOL_PERL": perl,
                   "EXIFTOOL_HOME": os.devnull, "PERL5LIB": "", "PERLLIB": "", "PERL5OPT": ""}
    environment.pop("EXIFTOOL", None)
    environment.pop("OXIDEX_ALLOW_EXIFTOOL_SKEW", None)
    oracle = [perl, f"-I{source / 'lib'}", str(source / "exiftool"), "-config", ""]
    subprocess.run([perl, "-MArchive::Zip", "-e", "1"], env=environment, check=True)
    for flags, expected in ((["-ver"], pin),
                            (["-s", "-s", "-s", "-FileType", str(source / "t/images/OOXML.docx")], "DOCX")):
        actual = subprocess.check_output([*oracle, *flags], env=environment, text=True).strip()
        if actual != expected:
            raise ValueError(f"Docs oracle probe expected {expected!r}, got {actual!r}")
    if (source / ".ExifTool_config").exists():
        raise ValueError("Pinned source contains an unexpected .ExifTool_config")
    cache.mkdir(parents=True, exist_ok=True)
    # Every run rebuilds the corpus from hashed archives, so a stale extracted
    # file cannot change the denominator. Runner scratch is isolated per run.
    with tempfile.TemporaryDirectory(prefix="docs-comparison-", dir=cache) as directory:
        work = Path(directory)
        shutil.copytree(source, work / "exiftool")
        # tag-comparison executes a named oracle directly and checks the
        # shebang interpreter's capabilities. Use the same proven Perl for
        # the wrapper and exec, with configuration disabled on every call.
        wrapper = work / "exiftool-docs"
        oracle_args = [perl, f"-I{source / 'lib'}", str(source / "exiftool"), "-config", ""]
        literals = ["'" + value.replace("\\", "\\\\").replace("'", "\\'") + "'"
                    for value in oracle_args]
        wrapper.write_text(f"#!{perl}\nexec " + ", ".join(literals) + ', @ARGV or die "exec: $!";\n')
        wrapper.chmod(0o700)
        corpus = work / "combined-samples"
        shutil.copytree(source / "t/images", corpus)
        for name, item in sorted(bootstrap.LOCK["archives"].items()):
            if not name.startswith("samples_"):
                continue
            archive = cache / item["filename"]
            if not archive.is_file() or bootstrap.sha256_file(archive) != item["sha256"]:
                partial = work / item["filename"]
                urls = (item["url"], "https://storage.googleapis.com/oxidex-samples/exiftool/"
                        + item["filename"].removeprefix("samples-"))
                for index, url in enumerate(urls):
                    try:
                        request = urllib.request.Request(url, headers={"User-Agent": "OxiDex/1.0"})
                        with urllib.request.urlopen(request, timeout=60) as response, partial.open("wb") as output:
                            shutil.copyfileobj(response, output)
                        if bootstrap.sha256_file(partial) != item["sha256"]:
                            raise ValueError(f"Locked archive hash mismatch: {item['filename']}")
                        break
                    except (OSError, ValueError):
                        if index == len(urls) - 1:
                            raise
                os.replace(partial, archive)
            with tarfile.open(archive) as contents:
                bootstrap._extract_safe_members(contents, corpus)
        bootstrap._verify_corpus_tree(corpus)
        environment["EXIFTOOL_CACHE_DIR"] = str(work)
        target = Path(environment.get("CARGO_TARGET_DIR", str(repo / "target")))
        if not target.is_absolute():
            target = repo / target
        subprocess.run(["cargo", "build", "--release", "--bin", "tag-comparison",
                        "--features", "tag-comparison-binary"], cwd=repo, env=environment, check=True)
        output = repo / "docs/reference/comparison"
        output.mkdir(parents=True, exist_ok=True)
        subprocess.run([str(target / "release/tag-comparison"),
                        "--exiftool", str(wrapper),
                        "--samples", str(corpus),
                        "--baseline", str(output / "baseline.json"),
                        "--output", str(output / "comparison.json"),
                        "--markdown-dir", str(output),
                        "--tag-cache-dir", str(work / "tag-cache"),
                        "--exiftool-version", pin], cwd=repo, env=environment, check=True)
        for report in (output / "comparison.json", output / "index.md"):
            if not report.is_file() or not report.stat().st_size:
                raise ValueError(f"Comparison did not produce {report}")


if __name__ == "__main__":
    try:
        generate(Path.cwd())
    except (ValueError, KeyError, OSError, bootstrap.Refused, subprocess.CalledProcessError) as exc:
        print(f"Docs comparison refused: {exc}", file=sys.stderr)
        raise SystemExit(1)
