"""Hermetic tests for validate_fix_commit.py.

Real `git` is exercised against throwaway tempdir repos (the spec's
Testing section explicitly asks for a real-git trailer round-trip), but
nothing else is real: the comparison runner is an injected function, the
samples cache / perl lib / squads.toml are tempdir fixtures, and no test
reads ~/.oxidex, /tmp/oxidex-exiftool-cache, or the live repo. Git's
global/system config is masked so a user's hooksPath/gpgsign settings
cannot leak into commit creation.
"""
import contextlib
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from validate_fix_commit import (
    check_ownership,
    extract_added_map_values,
    find_samples_carrying_tag,
    main,
    parse_trailers,
    squad_from_worker,
    validate_commit,
)

# Mask user/system git config (hooksPath, commit.gpgsign, ...) for every
# git call the fixtures make, so commit creation is hermetic.
GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_SYSTEM": os.devnull}


def git(repo, *args, input_text=None):
    return subprocess.run(
        ["git", *args],
        cwd=repo,
        input=input_text,
        capture_output=True,
        text=True,
        check=True,
        env=GIT_ENV,
    ).stdout


def make_repo(tmpdir):
    """Init a repo with one base commit so fix commits have clean diffs."""
    repo = Path(tmpdir) / "repo"
    repo.mkdir()
    git(repo, "init", "-q")
    git(repo, "config", "user.email", "fleet@example.com")
    git(repo, "config", "user.name", "Fleet Test")
    git(repo, "config", "commit.gpgsign", "false")
    (repo / "README.md").write_text("base\n")
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", "base commit")
    return repo


def full_trailers(**overrides):
    """A complete M1 trailer set; overrides replace keys, a None value
    drops the key entirely (for missing-trailer tests)."""
    trailers = {
        "Format": "JPEG",
        "Tag": ["EXIF:Sharpness"],
        "Sample": "/samples/canon1.jpg",
        "Exiftool-Value": "Normal",
        "Oxidex-Value": "Normal",
        "Perl-Ref": "Image/ExifTool/Canon.pm:1234",
        "Verified": "recheck-pass gaps=12->11",
        "Worker": "canon-1",
        "Table": "Canon::CameraSettings",
    }
    for key, value in overrides.items():
        if value is None:
            trailers.pop(key, None)
        else:
            trailers[key] = value
    return trailers


def commit_fix(repo, files, trailers, subject="fix: wire EXIF:Sharpness (JPEG)"):
    """Write files, commit them with a trailer block, return the sha."""
    for rel, content in files.items():
        path = repo / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
    git(repo, "add", "-A")
    lines = [subject, ""]
    for key, value in trailers.items():
        for item in value if isinstance(value, list) else [value]:
            lines.append(f"{key}: {item}")
    git(repo, "commit", "-q", "-m", "\n".join(lines))
    return git(repo, "rev-parse", "HEAD").strip()


def write_cache_file(cache_dir, name, entries):
    """One per-file exiftool output in the real exiftool-tag-cache shape."""
    payload = {"exiftool_version": "13.59", "result": {"tags": entries}}
    (cache_dir / f"{name}.json").write_text(json.dumps(payload))


def tag_entry(name, family, value, source_file):
    return {"name": name, "family": family, "value": value, "source_file": source_file}


def write_perl_module(perl_lib, body, name="Canon.pm"):
    module_dir = perl_lib / "Image" / "ExifTool"
    module_dir.mkdir(parents=True, exist_ok=True)
    (module_dir / name).write_text(body)
    return perl_lib


PERL_QUALITY = (
    "%canonQuality = (\n"
    "    1 => 'Economy',\n"
    "    2 => 'Normal',\n"
    "    3 => 'Fine',\n"
    ");\n"
)

RUST_QUALITY_OK = (
    "pub fn quality(v: u8) -> &'static str {\n"
    "    match v {\n"
    '        0x1 => "Economy",\n'
    '        0x2 => "Normal",\n'
    '        _ => "Fine",\n'
    "    }\n"
    "}\n"
)


class TrailerTests(unittest.TestCase):
    def test_round_trip_through_real_git_repo(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            sha = commit_fix(
                repo,
                {"src/canon/quality.rs": "pub fn noop() {}\n"},
                full_trailers(Tag=["EXIF:Sharpness", "MakerNotes:CanonQuality"]),
            )
            message = git(repo, "show", "-s", "--format=%B", sha)
            trailers = parse_trailers(message, repo)
            result = validate_commit(sha, repo)
        self.assertEqual(
            trailers["Tag"], ["EXIF:Sharpness", "MakerNotes:CanonQuality"]
        )
        self.assertEqual(trailers["Perl-Ref"], ["Image/ExifTool/Canon.pm:1234"])
        self.assertEqual(trailers["Verified"], ["recheck-pass gaps=12->11"])
        self.assertEqual(trailers["Worker"], ["canon-1"])
        self.assertEqual(result["checks"]["trailers"], "pass")
        self.assertTrue(result["ok"])

    def test_missing_required_trailer_is_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            sha = commit_fix(
                repo,
                {"src/canon/quality.rs": "pub fn noop() {}\n"},
                full_trailers(Worker=None, Table=None),
            )
            result = validate_commit(sha, repo)
        self.assertIn("missing-trailer:Worker", result["flags"])
        self.assertIn("missing-trailer:Table", result["flags"])
        self.assertEqual(result["checks"]["trailers"], "flagged")
        self.assertFalse(result["ok"])


class MultiSampleTests(unittest.TestCase):
    def _cache(self, tmp):
        cache = Path(tmp) / "cache"
        cache.mkdir()
        write_cache_file(
            cache,
            "canon",
            [
                tag_entry("Sharpness", "EXIF", "Normal", "/samples/canon1.jpg"),
                tag_entry("Sharpness", "EXIF", "Sharp", "/samples/canon2.jpg"),
            ],
        )
        write_cache_file(
            cache,
            "nikon",
            [
                tag_entry("Sharpness", "EXIF", "Soft", "/samples/nikon7.jpg"),
                # Same name under a different family must NOT count as a
                # carrier of EXIF:Sharpness.
                tag_entry("Sharpness", "MakerNotes", "2", "/samples/pentax1.jpg"),
            ],
        )
        return cache

    def test_finds_all_carriers_across_cache_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = self._cache(tmp)
            carriers = find_samples_carrying_tag(cache, "EXIF:Sharpness")
        self.assertEqual(
            carriers,
            ["/samples/canon1.jpg", "/samples/canon2.jpg", "/samples/nikon7.jpg"],
        )

    def test_all_carriers_matching_passes_and_every_carrier_is_compared(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            cache = self._cache(tmp)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": "pub fn noop() {}\n"}, full_trailers()
            )
            compared = []

            def comparison_fn(sample, tag):
                compared.append((sample, tag))
                return True

            result = validate_commit(
                sha, repo, samples_cache=cache, comparison_fn=comparison_fn
            )
        self.assertEqual(result["checks"]["multi_sample"], "pass")
        self.assertTrue(result["ok"])
        self.assertEqual(
            sorted(s for s, _ in compared),
            ["/samples/canon1.jpg", "/samples/canon2.jpg", "/samples/nikon7.jpg"],
        )

    def test_other_carrier_failing_flags_even_when_sample_file_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            cache = self._cache(tmp)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": "pub fn noop() {}\n"}, full_trailers()
            )
            # Matches on the commit's own Sample: file, wrong everywhere else
            # -- the class-(a) case the multi-sample check exists to catch.
            result = validate_commit(
                sha,
                repo,
                samples_cache=cache,
                comparison_fn=lambda sample, tag: sample == "/samples/canon1.jpg",
            )
        self.assertIn("multi-sample-fail:EXIF:Sharpness", result["flags"])
        self.assertEqual(result["checks"]["multi_sample"], "flagged")
        self.assertFalse(result["ok"])

    def test_tag_with_no_carriers_in_cache_is_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            cache = self._cache(tmp)
            sha = commit_fix(
                repo,
                {"src/canon/quality.rs": "pub fn noop() {}\n"},
                full_trailers(Tag=["EXIF:NoSuchTag"]),
            )
            result = validate_commit(
                sha, repo, samples_cache=cache, comparison_fn=lambda s, t: True
            )
        self.assertIn("multi-sample-no-carriers:EXIF:NoSuchTag", result["flags"])
        self.assertFalse(result["ok"])

    def test_skipped_without_cache_or_runner(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            cache = self._cache(tmp)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": "pub fn noop() {}\n"}, full_trailers()
            )
            no_cache = validate_commit(sha, repo, comparison_fn=lambda s, t: True)
            no_runner = validate_commit(sha, repo, samples_cache=cache)
        self.assertEqual(no_cache["checks"]["multi_sample"], "skipped")
        self.assertEqual(no_runner["checks"]["multi_sample"], "skipped")

    def test_plaintext_cache_file_falls_back_to_grep(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp) / "cache"
            cache.mkdir()
            (cache / "canon3.jpg.txt").write_text("Sharpness: Normal\nISO: 100\n")
            (cache / "nikon9.jpg.txt").write_text("ISO: 200\n")
            carriers = find_samples_carrying_tag(cache, "EXIF:Sharpness")
        self.assertEqual(carriers, [str(cache / "canon3.jpg")])


class PrintConvTests(unittest.TestCase):
    def test_values_present_in_perl_module_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": RUST_QUALITY_OK}, full_trailers()
            )
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertEqual(result["checks"]["printconv"], "pass")
        self.assertTrue(result["ok"])

    def test_byte_mismatch_is_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            rust = RUST_QUALITY_OK.replace('"Economy"', '"Economy mode"')
            sha = commit_fix(repo, {"src/canon/quality.rs": rust}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("printconv-mismatch:Economy mode", result["flags"])
        self.assertEqual(result["checks"]["printconv"], "flagged")
        self.assertFalse(result["ok"])

    def test_computed_value_is_flagged_unverifiable(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            rust = (
                "pub fn focal(v: u16) -> String {\n"
                "    match v {\n"
                '        0x0 => format!("{:.1} mm", 0.0),\n'
                "        _ => v.to_string(),\n"
                "    }\n"
                "}\n"
            )
            sha = commit_fix(repo, {"src/canon/focal.rs": rust}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("printconv-unverifiable", result["flags"])
        self.assertFalse(result["ok"])

    def test_values_with_no_perl_lib_are_unverifiable_not_silent(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": RUST_QUALITY_OK}, full_trailers()
            )
            result = validate_commit(sha, repo)  # no --perl-lib at all
        self.assertIn("printconv-unverifiable", result["flags"])
        self.assertFalse(result["ok"])

    def test_const_array_values_are_byte_checked(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(
                Path(tmp) / "perl", "%wb = (0 => 'Auto', 1 => 'Daylight');\n"
            )
            rust = (
                "const WHITE_BALANCE: [&str; 3] = [\n"
                '    "Auto",\n'
                '    "Daylight",\n'
                '    "Fluorescent-ish",\n'
                "];\n"
            )
            sha = commit_fix(repo, {"src/canon/wb.rs": rust}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("printconv-mismatch:Fluorescent-ish", result["flags"])
        self.assertNotIn("printconv-mismatch:Auto", result["flags"])

    def test_extractor_only_takes_added_map_values(self):
        diff = (
            "diff --git a/src/x.rs b/src/x.rs\n"
            "--- a/src/x.rs\n"
            "+++ b/src/x.rs\n"
            "@@ -1,4 +1,6 @@\n"
            " const NAMES: [&str; 9] = [\n"
            '     "Existing",\n'
            '+    "Inserted",\n'
            " ];\n"
            '-    0x1 => "Removed",\n'
            '+    0x1 => "Added",\n'
            '+    "Key" => "RhsOnly",\n'
            '+    let s = "not a map value";\n'
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, ["Inserted", "Added", "RhsOnly"])
        self.assertEqual(unverifiable, [])

    def test_extractor_ignores_benign_numeric_rhs(self):
        diff = "diff --git a/s b/s\n+        0x1 => 3,\n+        0x2 => 0x1f,\n"
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, [])
        self.assertEqual(unverifiable, [])

    def test_extractor_catches_bare_string_inserted_mid_table(self):
        # The declaring `const ... = [` line is OUTSIDE the hunk (git only
        # shows neighboring entries as context), so bracket-depth tracking
        # never opens -- the added bare-string element must still be
        # treated as a map value, or a fabricated value inserted into the
        # middle of a big existing table passes silently.
        diff = (
            "diff --git a/src/canon/lens.rs b/src/canon/lens.rs\n"
            "--- a/src/canon/lens.rs\n"
            "+++ b/src/canon/lens.rs\n"
            "@@ -100,6 +100,7 @@ const LENS_NAMES: [&str; 400] = [\n"
            '     "Canon EF 50mm f/1.8",\n'
            '+    "Fabricated Lens Name That Is Not In Perl",\n'
            '     "Canon EF 85mm f/1.8",\n'
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, ["Fabricated Lens Name That Is Not In Perl"])
        self.assertEqual(unverifiable, [])

    def test_extractor_takes_multiple_bare_strings_per_line(self):
        diff = 'diff --git a/s b/s\n+    "One", "Two",\n'
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["One", "Two"])

    def test_mid_table_fabricated_value_is_flagged_end_to_end(self):
        base = (
            "const LENS_NAMES: [&str; 8] = [\n"
            '    "Canon EF 20mm f/2.8 USM",\n'
            '    "Canon EF 24mm f/2.8",\n'
            '    "Canon EF 28mm f/2.8",\n'
            '    "Canon EF 35mm f/2",\n'
            '    "Canon EF 50mm f/1.8",\n'
            '    "Canon EF 85mm f/1.8 USM",\n'
            '    "Canon EF 100mm f/2 USM",\n'
            '    "Canon EF 135mm f/2L USM",\n'
            "];\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(
                Path(tmp) / "perl",
                "%lens = (1 => 'Canon EF 50mm f/1.8', "
                "2 => 'Canon EF 300mm f/4L IS USM');\n",
            )
            commit_fix(repo, {"src/canon/lens.rs": base}, full_trailers(),
                       subject="base: lens table")
            fabricated = base.replace(
                '    "Canon EF 85mm f/1.8 USM",\n',
                '    "Fabricated Lens Name That Is Not In Perl",\n'
                '    "Canon EF 85mm f/1.8 USM",\n')
            sha = commit_fix(repo, {"src/canon/lens.rs": fabricated}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn(
            "printconv-mismatch:Fabricated Lens Name That Is Not In Perl",
            result["flags"])
        self.assertEqual(result["checks"]["printconv"], "flagged")
        self.assertFalse(result["ok"])

        # ...and the same shape with a genuine Perl value passes clean.
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(
                Path(tmp) / "perl",
                "%lens = (1 => 'Canon EF 50mm f/1.8', "
                "2 => 'Canon EF 300mm f/4L IS USM');\n",
            )
            commit_fix(repo, {"src/canon/lens.rs": base}, full_trailers(),
                       subject="base: lens table")
            genuine = base.replace(
                '    "Canon EF 85mm f/1.8 USM",\n',
                '    "Canon EF 300mm f/4L IS USM",\n'
                '    "Canon EF 85mm f/1.8 USM",\n')
            sha = commit_fix(repo, {"src/canon/lens.rs": genuine}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertEqual(result["checks"]["printconv"], "pass")
        self.assertTrue(result["ok"])


class OwnershipTests(unittest.TestCase):
    def _squads_toml(self, tmp):
        path = Path(tmp) / "squads.toml"
        path.write_text(
            "[squads.canon]\n"
            'files = ["src/canon/*", "oxidex-tags-canon/*"]\n'
            "[squads.nikon]\n"
            'files = ["src/nikon/*"]\n'
        )
        return path

    def test_squad_from_worker_strips_trailing_index_only(self):
        self.assertEqual(squad_from_worker("canon-2"), "canon")
        self.assertEqual(squad_from_worker("sony-minolta-11"), "sony-minolta")
        self.assertEqual(squad_from_worker("canon"), "canon")

    def test_out_of_squad_file_warns_but_never_hard_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            squads = self._squads_toml(tmp)
            sha = commit_fix(
                repo,
                {
                    "src/canon/quality.rs": "pub fn noop() {}\n",
                    "src/other/mod.rs": "pub fn stray() {}\n",
                },
                full_trailers(),
            )
            result = validate_commit(sha, repo, squads_toml=squads)
        self.assertIn("ownership:src/other/mod.rs", result["flags"])
        self.assertNotIn("ownership:src/canon/quality.rs", result["flags"])
        self.assertEqual(result["checks"]["ownership"], "warn")
        # WARN-ONLY per spec M1: the commit is still ok/clean.
        self.assertTrue(result["ok"])

    def test_in_squad_files_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            squads = self._squads_toml(tmp)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": "pub fn noop() {}\n"}, full_trailers()
            )
            result = validate_commit(sha, repo, squads_toml=squads)
        self.assertEqual(result["checks"]["ownership"], "pass")
        self.assertEqual(result["flags"], [])

    def test_skipped_without_manifest_or_unknown_squad(self):
        status, flags = check_ownership(["src/x.rs"], "canon-1", {})
        self.assertEqual((status, flags), ("skipped", []))
        status, flags = check_ownership(
            ["src/x.rs"], "thermal-3", {"canon": ["src/canon/*"]}
        )
        self.assertEqual((status, flags), ("skipped", []))


class PatchIdAndCliTests(unittest.TestCase):
    def _run_main(self, argv):
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            rc = main(argv)
        return rc, stdout.getvalue(), stderr.getvalue()

    def test_patch_id_is_computed_and_stable_shaped(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": "pub fn noop() {}\n"}, full_trailers()
            )
            result = validate_commit(sha, repo)
        self.assertRegex(result["patch_id"], r"^[0-9a-f]{40}$")

    def test_json_output_and_exit_zero_when_clean(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": "pub fn noop() {}\n"}, full_trailers()
            )
            rc, out, _ = self._run_main([sha, "--repo", str(repo), "--json"])
            parsed = json.loads(out)
        self.assertEqual(rc, 0)
        self.assertTrue(parsed["ok"])
        self.assertEqual(parsed["flags"], [])
        self.assertRegex(parsed["patch_id"], r"^[0-9a-f]{40}$")
        self.assertEqual(
            set(parsed["checks"]),
            {"trailers", "multi_sample", "printconv", "ownership"},
        )

    def test_exit_two_when_flagged(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            sha = commit_fix(
                repo,
                {"src/canon/quality.rs": "pub fn noop() {}\n"},
                full_trailers(Verified=None),
            )
            rc, out, _ = self._run_main([sha, "--repo", str(repo), "--json"])
            parsed = json.loads(out)
        self.assertEqual(rc, 2)
        self.assertIn("missing-trailer:Verified", parsed["flags"])

    def test_exit_one_on_unknown_sha(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            rc, _, err = self._run_main(["deadbeef" * 5, "--repo", str(repo), "--json"])
        self.assertEqual(rc, 1)
        self.assertIn("error:", err)

    def test_exit_one_on_missing_samples_cache_dir(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": "pub fn noop() {}\n"}, full_trailers()
            )
            rc, _, err = self._run_main(
                [
                    sha,
                    "--repo",
                    str(repo),
                    "--samples-cache",
                    str(Path(tmp) / "nope"),
                    "--comparison-cmd",
                    "true",
                ]
            )
        self.assertEqual(rc, 1)
        self.assertIn("error:", err)

    def test_comparison_cmd_path_end_to_end(self):
        """--comparison-cmd wiring: a tiny always-match script stands in
        for the tag-comparison binary; no cargo, no network."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            cache = Path(tmp) / "cache"
            cache.mkdir()
            write_cache_file(
                cache,
                "canon",
                [tag_entry("Sharpness", "EXIF", "Normal", "/samples/canon1.jpg")],
            )
            compare = Path(tmp) / "compare.sh"
            compare.write_text("#!/bin/sh\nexit 0\n")
            compare.chmod(0o755)
            sha = commit_fix(
                repo, {"src/canon/quality.rs": "pub fn noop() {}\n"}, full_trailers()
            )
            rc, out, _ = self._run_main(
                [
                    sha,
                    "--repo",
                    str(repo),
                    "--samples-cache",
                    str(cache),
                    "--comparison-cmd",
                    str(compare),
                    "--json",
                ]
            )
            parsed = json.loads(out)
        self.assertEqual(rc, 0)
        self.assertEqual(parsed["checks"]["multi_sample"], "pass")


if __name__ == "__main__":
    unittest.main()
