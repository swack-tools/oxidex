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
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

import validate_fix_commit
from validate_fix_commit import (
    check_trailer_truth,
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


class TrailerTruthTests(unittest.TestCase):
    """A trailer can be PRESENT and FALSE.

    On 2026-07-27 a JPEG fix passed the whole gate citing
    `Perl-Ref: NikonCustom.pm` for six APP12 tags that module does not
    define. These pin the discriminator that tells a wrong citation from a
    right one -- see check_trailer_truth and _defines_tag.
    """

    def _lib(self, tmp, **modules):
        lib = Path(tmp) / "lib" / "Image" / "ExifTool"
        lib.mkdir(parents=True)
        for name, body in modules.items():
            (lib / f"{name}.pm").write_text(body)
        return Path(tmp) / "lib"

    def test_module_defining_the_tag_is_accepted(self):
        with tempfile.TemporaryDirectory() as tmp:
            lib = self._lib(tmp, APP12="%Table = (\n    Protect     => { },\n);\n")
            self.assertEqual(
                check_trailer_truth(
                    {"Tag": ["APP12:Protect"], "Perl-Ref": ["APP12.pm"]}, perl_lib=lib
                ),
                [],
            )

    def test_module_that_only_MENTIONS_the_name_is_rejected(self):
        # The exact shape that made presence-checking useless: NikonCustom.pm
        # holds `24 => 'Protect'`, a custom-setting VALUE that collides with
        # the tag name. Both modules "mention Protect"; only one defines it.
        with tempfile.TemporaryDirectory() as tmp:
            lib = self._lib(
                tmp,
                APP12="%Table = (\n    Protect     => { },\n);\n",
                NikonCustom="PrintConv => {\n        24 => 'Protect',\n    },\n",
            )
            self.assertEqual(
                check_trailer_truth(
                    {"Tag": ["APP12:Protect"], "Perl-Ref": ["NikonCustom.pm"]},
                    perl_lib=lib,
                ),
                ["perl-ref-documents-none:NikonCustom.pm"],
            )

    def test_hex_keyed_table_naming_the_tag_counts_as_defining_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            lib = self._lib(
                tmp,
                Exif="    0xa402 => {\n        Name => 'ExposureMode',\n    },\n",
            )
            self.assertEqual(
                check_trailer_truth(
                    {"Tag": ["EXIF:ExposureMode"], "Perl-Ref": ["Exif.pm"]},
                    perl_lib=lib,
                ),
                [],
            )

    def test_runtime_named_tags_never_flag_a_correct_module(self):
        # ExifTool names some tags at runtime (ProcessAPP12's `ucfirst $tag`
        # produces REV/STB1), so they appear in NO table anywhere. A commit
        # fixing only those must not be flagged against a correct Perl-Ref --
        # nothing in the corpus can disprove the citation.
        with tempfile.TemporaryDirectory() as tmp:
            lib = self._lib(tmp, APP12="sub ProcessAPP12 { ucfirst $tag }\n")
            self.assertEqual(
                check_trailer_truth(
                    {"Tag": ["APP12:REV", "APP12:STB1"], "Perl-Ref": ["APP12.pm"]},
                    perl_lib=lib,
                ),
                [],
            )

    def test_no_perl_lib_yields_no_flag(self):
        # Conservative in the same direction as the rest of the module: an
        # absent corpus cannot disprove anything, so it must not accuse.
        self.assertEqual(
            check_trailer_truth(
                {"Tag": ["APP12:Protect"], "Perl-Ref": ["NikonCustom.pm"]},
                perl_lib=None,
            ),
            [],
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
                full_trailers(Worker=None),
            )
            result = validate_commit(sha, repo)
        self.assertIn("missing-trailer:Worker", result["flags"])
        self.assertEqual(result["checks"]["trailers"], "flagged")
        self.assertFalse(result["ok"])

    def test_missing_table_trailer_alone_is_not_flagged(self):
        # Table is a T3 table-port-job concept; model_fix_loop.py's
        # ordinary fix_gap commits never populate it (table_name=None at
        # every regular call site) -- an ordinary fix missing ONLY this
        # trailer must still validate clean, or no ordinary fix could
        # ever be auto-consumed.
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            sha = commit_fix(
                repo,
                {"src/canon/quality.rs": "pub fn noop() {}\n"},
                full_trailers(Table=None),
            )
            result = validate_commit(sha, repo)
        self.assertNotIn("missing-trailer:Table", result["flags"])
        self.assertEqual(result["checks"]["trailers"], "pass")


class PrintConvIdentifierExclusionTests(unittest.TestCase):
    """A PrintConv value is a human-readable DISPLAY string. A tag key, a
    byte-order magic constant, and a tag-name registry entry are
    identifiers -- they are never expected to appear in an ExifTool
    module's PrintConv tables, so demanding they do rejects correct code.

    Measured live 2026-07-25: this was the single largest quarantine
    cause after the Table-trailer fix (15 of 27 flags), and it rejected a
    valid DNG fix whose diff was a tag-ID -> tag-NAME registry.
    """

    def test_tag_key_map_values_are_not_treated_as_printconv(self):
        from validate_fix_commit import extract_added_map_values
        diff = (
            "diff --git a/src/x.rs b/src/x.rs\n"
            "@@ -1 +1 @@\n"
            '+            0x0111 => "EXIF:PreviewImageStart".to_string(),\n'
            '+            0x0143 => "EXIF:TileLength".to_string(),\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, [])

    def test_byte_string_magic_is_not_treated_as_printconv(self):
        from validate_fix_commit import extract_added_map_values
        diff = (
            "diff --git a/src/x.rs b/src/x.rs\n"
            "@@ -1 +1 @@\n"
            '+        ByteOrder::LittleEndian => tiff.extend_from_slice(b"II\\x2a\\x00"),\n'
            '+        ByteOrder::BigEndian => tiff.extend_from_slice(b"MM\\x00\\x2a"),\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, [])

    def test_a_real_printconv_value_is_still_extracted(self):
        # The gate must keep doing its job: a genuine display string on
        # the right of => is still checked byte-for-byte.
        from validate_fix_commit import extract_added_map_values
        diff = (
            "diff --git a/src/x.rs b/src/x.rs\n"
            "@@ -1 +1 @@\n"
            '+            1 => "Intel 386 or later, and compatibles",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertIn("Intel 386 or later, and compatibles", values)

    def test_a_fabricated_display_value_is_still_caught_end_to_end(self):
        from validate_fix_commit import check_printconv
        with tempfile.TemporaryDirectory() as tmp:
            lib = Path(tmp) / "Image" / "ExifTool"
            lib.mkdir(parents=True)
            (lib / "Canon.pm").write_text("package X;\n1 => 'Economy',\n")
            diff = (
                "diff --git a/src/x.rs b/src/x.rs\n"
                "@@ -1 +1 @@\n"
                '+            1 => "Economy mode",\n'
            )
            status, flags = check_printconv(diff, "Canon.pm", Path(tmp))
        self.assertEqual(status, "flagged")
        self.assertTrue(any(f.startswith("printconv-mismatch:Economy mode") for f in flags))

    def test_tag_key_shape_requires_both_halves_to_be_identifiers(self):
        # "Fine: Best" is a plausible display string, not a tag key --
        # the exclusion must not swallow it.
        from validate_fix_commit import looks_like_tag_key
        self.assertTrue(looks_like_tag_key("EXIF:PreviewImageStart"))
        self.assertTrue(looks_like_tag_key("MakerNotes:AELockButton"))
        self.assertFalse(looks_like_tag_key("Fine: Best"))
        self.assertFalse(looks_like_tag_key("Disable; 0; 8; 0"))
        self.assertFalse(looks_like_tag_key("Normal"))
        self.assertFalse(looks_like_tag_key("1/250"))


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
            {"trailers", "multi_sample", "printconv", "paths", "ownership"},
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


class FalseQuarantineRegressionTests(unittest.TestCase):
    """The four extractor defects that produced 33 of the 77 quarantined
    heads measured 2026-07-25, plus the guards that keep fixing them from
    re-opening the fabricated-value hole they exist to close."""

    def setUp(self):
        # The perl-lib corpus is cached per path for sweep throughput;
        # tempdir fixtures reuse paths across tests in the same process.
        validate_fix_commit._perl_lib_corpus.cache_clear()

    # -- match arms are not map entries ---------------------------------

    def test_stringless_match_arm_is_not_unverifiable(self):
        # `=>` is match-arm syntax in Rust, and a match arm dispatching a
        # tag id to a decoder is the commonest shape of a tag-wiring fix.
        # A right-hand side with no string cannot hide a fabricated
        # display value.
        diff = (
            "diff --git a/src/raw/metadata.rs b/src/raw/metadata.rs\n"
            "@@ -10,3 +10,7 @@ fn read_header(bytes: &[u8]) -> Result<()> {\n"
            "+        ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),\n"
            "+        ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),\n"
            "+        0x0100 => thumb_width = read_tiff_u32(bytes, byte_order),\n"
            "+        Err(_) => return,\n"
            "+        _ => continue,\n"
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, [])
        self.assertEqual(unverifiable, [])

    def test_real_printconv_arm_is_still_extracted(self):
        diff = (
            "diff --git a/src/canon/q.rs b/src/canon/q.rs\n"
            "@@ -1,2 +1,3 @@ fn quality(v: u8) -> &'static str {\n"
            '+        0x1 => "Economy",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Economy"])

    # -- &[&str] key registries are identifiers, not values -------------

    def test_str_slice_registry_elements_are_not_printconv_values(self):
        # const KNOWN_TAGS: &[&str] = &[...] is a registry of raw APP12
        # KEY NAMES the parser recognises. Demanding "REV"/"S0"/"STB1"
        # appear in an ExifTool PrintConv table rejects correct code.
        diff = (
            "diff --git a/src/jpeg/app12_olympus.rs b/src/jpeg/app12_olympus.rs\n"
            "@@ -102,6 +102,10 @@ const KNOWN_TAGS: &[&str] = &[\n"
            '     "Protect",\n'
            '+    "REV",\n'
            '+    "S0",\n'
            '+    "STB1",\n'
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, [])
        self.assertEqual(unverifiable, [])

    def test_fixed_size_str_array_is_still_checked(self):
        # `[&str; 400]` is the INDEXED PrintConv lookup idiom -- same
        # element type as the registry above, but its elements really are
        # display values. The `;` is the discriminator.
        diff = (
            "diff --git a/src/canon/lens.rs b/src/canon/lens.rs\n"
            "@@ -100,6 +100,7 @@ const LENS_NAMES: [&str; 400] = [\n"
            '     "Canon EF 50mm f/1.8",\n'
            '+    "Fabricated Lens Name",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Fabricated Lens Name"])

    def test_unparseable_hunk_context_still_checks_bare_strings(self):
        # git's default funcname driver only reports column-0
        # declarations, so a const declared indented inside an fn/impl
        # shows NO useful context. The registry rule is a negative gate
        # precisely so this case keeps its fabrication check.
        diff = (
            "diff --git a/src/canon/lens.rs b/src/canon/lens.rs\n"
            "@@ -100,6 +100,7 @@ impl LensResolver {\n"
            '     "Canon EF 50mm f/1.8",\n'
            '+    "Fabricated Lens Name",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Fabricated Lens Name"])

    # -- format! templates are not literals -----------------------------

    def test_format_macro_arm_is_unverifiable_not_fabricated(self):
        diff = (
            "diff --git a/src/x.rs b/src/x.rs\n"
            "@@ -1,2 +1,3 @@ fn describe(other: u32) -> String {\n"
            '+        other => format!("Unknown({})", other),\n'
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, [])
        self.assertEqual(len(unverifiable), 1)

    def test_standalone_format_template_line_is_not_a_value(self):
        # A multi-line format!( ... ) call whose template sits alone on
        # its own line looks exactly like a bare table element.
        diff = (
            "diff --git a/src/jpeg/flir_parser.rs b/src/jpeg/flir_parser.rs\n"
            "@@ -40,3 +40,5 @@ fn parse_flir_datetime(raw: &[u8]) -> String {\n"
            '+        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:03}",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, [])

    # -- test code is not production evidence ---------------------------

    def test_assert_messages_in_test_modules_are_ignored(self):
        diff = (
            "diff --git a/src/thermal.rs b/src/thermal.rs\n"
            "@@ -200,3 +200,5 @@ mod tests {\n"
            '+        assert_eq!(v, "Off", "Flash=0 should be PrintConv\'d to Off");\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, [])

    # -- wrong module cited != invented value ---------------------------

    def test_value_in_another_module_is_warn_only_not_mismatch(self):
        rust = (
            "pub fn quality(v: u8) -> &'static str {\n"
            "    match v {\n"
            '        0x1 => "Co-sited",\n'
            '        _ => "Unknown",\n'
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            # The cited module (Canon.pm) lacks the value; Exif.pm has it.
            write_perl_module(perl_lib, "%ycc = (2 => 'Co-sited');\n", name="Exif.pm")
            sha = commit_fix(repo, {"src/canon/q.rs": rust}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("printconv-wrong-perl-ref:Co-sited", result["flags"])
        self.assertNotIn("printconv-mismatch:Co-sited", result["flags"])

    def test_value_in_no_module_at_all_is_still_a_hard_mismatch(self):
        rust = (
            "pub fn quality(v: u8) -> &'static str {\n"
            "    match v {\n"
            '        0x1 => "Totally Invented Value",\n'
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            sha = commit_fix(repo, {"src/canon/q.rs": rust}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("printconv-mismatch:Totally Invented Value", result["flags"])
        self.assertFalse(result["ok"])

    # -- Perl-Ref is required only when it is consumed ------------------

    def test_perl_ref_not_required_when_diff_has_no_printconv_value(self):
        # Pure wiring: a tag with no Perl table block behind it. The
        # emitter omits Perl-Ref by design, so requiring it quarantined
        # the fix forever.
        rust = (
            "pub fn wire(bytes: &[u8]) {\n"
            "    match tag {\n"
            "        0x0100 => width = read_u32(bytes),\n"
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            sha = commit_fix(
                repo, {"src/raw/wire.rs": rust}, full_trailers(**{"Perl-Ref": None})
            )
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertNotIn("missing-trailer:Perl-Ref", result["flags"])

    def test_perl_ref_still_required_when_there_is_a_value_to_attest(self):
        rust = (
            "pub fn quality(v: u8) -> &'static str {\n"
            "    match v {\n"
            '        0x1 => "Economy",\n'
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            sha = commit_fix(
                repo, {"src/canon/q.rs": rust}, full_trailers(**{"Perl-Ref": None})
            )
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("missing-trailer:Perl-Ref", result["flags"])
        self.assertFalse(result["ok"])


class LooseningHardeningTests(unittest.TestCase):
    """Regressions introduced by the 2026-07-25 extractor loosening
    (POLICY_VERSION 4) and closed at POLICY_VERSION 5. Each was
    independently reproduced against the pre-loosening validator, which
    rejected the same commit."""

    def setUp(self):
        validate_fix_commit._perl_lib_corpus.cache_clear()

    # -- the mod-tests hunk gate was exploitable ------------------------

    def test_a_fabrication_appended_at_eof_is_not_excused_by_a_mod_tests_header(self):
        # git's default funcname driver reports the nearest preceding
        # COLUMN-0 declaration, and `#[cfg(test)] mod tests {` is
        # conventionally the LAST one in a Rust file. So an end-of-file
        # append gets `mod tests` as its hunk context while being 100%
        # production code. The old hunk-level gate skipped all of it.
        diff = (
            "diff --git a/src/core/formatters/exposure_program.rs "
            "b/src/core/formatters/exposure_program.rs\n"
            "@@ -141,3 +141,7 @@ mod tests {\n"
            "+pub fn program(v: u8) -> &'static str {\n"
            "+    match v {\n"
            '+        1 => "Landscape Mode",\n'
            '+        2 => "Night Scene Mode",\n'
            "+    }\n"
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Landscape Mode", "Night Scene Mode"])

    def test_an_assert_message_is_still_ignored(self):
        # The real false positive the hunk gate was introduced for.
        diff = (
            "diff --git a/src/thermal.rs b/src/thermal.rs\n"
            "@@ -200,3 +200,5 @@ mod tests {\n"
            '+        assert_eq!(v, "Off", "Flash=0 should be PrintConv\'d to Off");\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, [])

    def test_a_multiline_assert_message_is_ignored_too(self):
        # rustfmt splits long asserts, leaving the message alone on a line
        # with no macro token on it. Measured on 12a20366f5bc.
        diff = (
            "diff --git a/src/thermal.rs b/src/thermal.rs\n"
            "@@ -200,3 +200,8 @@ mod tests {\n"
            "+        assert_eq!(\n"
            '+            metadata.get_string("APP12:Flash"),\n'
            '+            Some("Off"),\n'
            "+            \"Flash=0 should be PrintConv'd to Off\"\n"
            "+        );\n"
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, [])

    def test_a_fabricated_value_after_a_multiline_assert_is_still_caught(self):
        # The assert skip must CLOSE, not swallow the rest of the hunk.
        diff = (
            "diff --git a/src/canon/q.rs b/src/canon/q.rs\n"
            "@@ -10,3 +10,8 @@ fn quality(v: u8) -> &'static str {\n"
            "+        assert_eq!(\n"
            '+            got,\n'
            '+            Some("Off"),\n'
            "+        );\n"
            '+        0x1 => "Fabricated Display Value",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Fabricated Display Value"])

    # -- the registry gate keys on the NAME, not the type shape ---------

    def test_an_icc_display_value_slice_is_still_checked(self):
        # RENDERING_INTENTS is `&[&str]` but is an INDEXED PrintConv
        # table; the shape-based gate skipped it.
        diff = (
            "diff --git a/src/parsers/icc/registries.rs b/src/parsers/icc/registries.rs\n"
            "@@ -322,4 +322,5 @@ pub static RENDERING_INTENTS: &[&str] = &[\n"
            '     "Perceptual",\n'
            '+    "Fabricated Intent",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Fabricated Intent"])

    def test_a_named_tag_registry_slice_is_still_skipped(self):
        # The case the loosening existed for must keep working.
        diff = (
            "diff --git a/src/jpeg/app12_olympus.rs b/src/jpeg/app12_olympus.rs\n"
            "@@ -102,6 +102,9 @@ const KNOWN_TAGS: &[&str] = &[\n"
            '     "Protect",\n'
            '+    "REV",\n'
            '+    "STB1",\n'
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, [])
        self.assertEqual(unverifiable, [])

    # -- a real string in the wrong module must still BLOCK -------------

    def test_wrong_perl_ref_is_labelled_but_still_blocks(self):
        rust = (
            "pub fn quality(v: u8) -> &'static str {\n"
            "    match v {\n"
            '        0x1 => "Co-sited",\n'
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            write_perl_module(perl_lib, "%ycc = (2 => 'Co-sited');\n", name="Exif.pm")
            sha = commit_fix(repo, {"src/canon/q.rs": rust}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        # Still named distinctly, so a human sees "real string, wrong
        # module" rather than "invented string" ...
        self.assertIn("printconv-wrong-perl-ref:Co-sited", result["flags"])
        # ... but it no longer auto-admits.
        self.assertFalse(result["ok"])

    def test_the_corpus_second_opinion_ignores_lang_translation_tables(self):
        # Image/ExifTool/Lang/*.pm carries every display string in every
        # supported language and would rescue almost any fabrication.
        with tempfile.TemporaryDirectory() as tmp:
            perl_lib = Path(tmp) / "perl"
            write_perl_module(perl_lib, "%q = (1 => 'Real');\n")
            lang = perl_lib / "Image" / "ExifTool" / "Lang"
            lang.mkdir(parents=True, exist_ok=True)
            (lang / "de.pm").write_text("%de = ('Totally Invented' => 'Erfunden');\n")
            validate_fix_commit._perl_lib_corpus.cache_clear()
            corpus = validate_fix_commit._perl_lib_corpus(perl_lib)
        self.assertIn(b"Real", corpus)
        self.assertNotIn(b"Totally Invented", corpus)

    def test_policy_version_was_bumped_so_the_old_verdicts_are_re_examined(self):
        # Heads admitted under the loosened policy 4 must be reconsidered.
        self.assertGreaterEqual(validate_fix_commit.POLICY_VERSION, 5)


class NonSourceFilePathTests(unittest.TestCase):
    """A tag fix that also commits a stray artifact from the worker's
    worktree must not reach main. Measured case: 85a24f04390d on
    model-fix-parallel-standards-appn-1 added config.toml.bak-pre-gpt55
    (163 lines) beside a real fix and validated CLEAN."""

    def test_a_committed_config_backup_blocks_the_fix(self):
        rust = (
            "pub fn quality(v: u8) -> &'static str {\n"
            "    match v {\n"
            '        0x1 => "Economy",\n'
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            sha = commit_fix(
                repo,
                {
                    "src/canon/q.rs": rust,
                    "config.toml.bak-pre-gpt55": "[worker]\nmodel = 'x'\n",
                },
                full_trailers(),
            )
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("non-source-file:config.toml.bak-pre-gpt55", result["flags"])
        self.assertFalse(result["ok"], "a stray artifact must BLOCK, not warn")

    def test_a_clean_fix_is_unaffected(self):
        rust = (
            "pub fn quality(v: u8) -> &'static str {\n"
            "    match v {\n"
            '        0x1 => "Economy",\n'
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            sha = commit_fix(repo, {"src/canon/q.rs": rust}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertEqual([f for f in result["flags"] if f.startswith("non-source")], [])
        self.assertTrue(result["ok"])

    def test_every_place_a_real_tag_fix_writes_is_allowed(self):
        # Derived from the 17 distinct paths every worker fix commit
        # currently ahead of origin/main touches, plus the manifest and
        # crate areas a fix could legitimately need.
        for path in (
            "src/parsers/jpeg/flir_parser.rs",
            "src/parsers/tiff/makernotes/canon.rs",
            "tests/integration_jpeg.rs",
            "docs/tag-coverage.md",
            "benches/parse.rs",
            "bindings/c/oxidex.h",
            "oxidex-tags-core/src/lib.rs",
            "oxidex-tags-camera/src/canon.rs",
            "Cargo.toml",
            "Cargo.lock",
        ):
            with self.subTest(path=path):
                self.assertTrue(validate_fix_commit.is_fix_commit_path(path))

    def test_stray_artifacts_and_non_tag_fix_areas_are_rejected(self):
        for path in (
            "config.toml.bak-pre-gpt55",
            "config.toml.bak-medium",
            "config.example.toml",
            ".mcp.json",
            ".gitignore",
            "justfile",
            ".githooks/pre-commit",
            # Fleet-infrastructure commits are not tag fixes. Two of them
            # (7a5dd662, 93994f59) were routed through this validator and
            # written into all 14 squads' ledgers, producing 28 of the 77
            # quarantine entries as eight misleading missing-trailer flags
            # apiece.
            "scripts/model_fix_loop.py",
            "scripts/parallel_model_fix_loop.py",
            # A repo-root file that merely starts with the tag-crate
            # prefix is not a tag crate.
            "oxidex-tags-notes.bak",
        ):
            with self.subTest(path=path):
                self.assertFalse(validate_fix_commit.is_fix_commit_path(path))

    def test_the_flag_is_hard_not_warn_only(self):
        self.assertFalse("non-source-file:".startswith(
            validate_fix_commit.WARN_ONLY_FLAG_PREFIXES))


class ExtractorEvasionShapeTests(unittest.TestCase):
    """Shapes that carried a display string straight past the PrintConv
    byte check (measured on origin/main = a2aa0df, POLICY_VERSION 5,
    2026-07-26). Each was reproduced end-to-end before being fixed:
    extract_added_map_values returned ([], []), validate_commit returned
    ok=True with no flags, and overlord_sweep.classify_for_judgment_queue
    -- which shares this extractor and is the only thing that routes a
    commit to a human -- returned [] as well. So the commit shipped as
    machine_accepted with zero review of a value nothing had verified.

    The three families:
      * a BLOCK match arm (`260 => {` / `"...".to_string()` / `}`) --
        rustfmt MANUFACTURES this shape out of a same-line arm at 71+
        chars of display value (measured below), so the check switched
        itself off as a pure function of line width;
      * the repo's own table macro (`const_decoder!(...[(12, "..."), ...])`,
        338 uses across 46 files) and `.insert(k, "...")`, neither of
        which has a `=>` or is a bare string element;
      * a TRAILING COMMENT on a bare table element -- the file's own
        house style (`"F2",             // 4`) -- which broke the
        `$`-anchored bare-string rule.
    """

    def setUp(self):
        validate_fix_commit._perl_lib_corpus.cache_clear()

    # -- block match arms (rustfmt's own output) -------------------------

    def test_block_arm_body_value_is_extracted(self):
        diff = (
            "diff --git a/src/canon/lens.rs b/src/canon/lens.rs\n"
            "@@ -20,6 +20,9 @@ pub fn lens_type(v: u16) -> String {\n"
            "+        260 => {\n"
            '+            "Fabricated Sigma 150-600mm F5-6.3 DG OS HSM Sport".to_string()\n'
            "+        }\n"
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(
            values, ["Fabricated Sigma 150-600mm F5-6.3 DG OS HSM Sport"])
        self.assertEqual(unverifiable, [])

    def test_block_arm_conversion_wrappers_are_extracted(self):
        # Any expression wrapped around the literal used to defeat the
        # bare-string rule; these are the wrappers real oxidex code uses.
        for body in (
            '            "Fabricated Value".to_string()',
            '            "Fabricated Value".into()',
            '            String::from("Fabricated Value")',
            '            Some("Fabricated Value".to_string())',
            '            Cow::Borrowed("Fabricated Value")',
            '            TagValue::String("Fabricated Value".to_owned())',
            '            return "Fabricated Value".to_string();',
        ):
            with self.subTest(body=body.strip()):
                diff = (
                    "diff --git a/src/canon/lens.rs b/src/canon/lens.rs\n"
                    "@@ -20,6 +20,9 @@ pub fn lens_type(v: u16) -> String {\n"
                    "+        260 => {\n"
                    f"+{body}\n"
                    "+        }\n"
                )
                values, _ = extract_added_map_values(diff)
                self.assertEqual(values, ["Fabricated Value"])

    # shutil.which rather than `subprocess.run(["which", ...])`: it answers
    # the same question without spawning a process, so Bandit's B603
    # subprocess warning never arises and there is nothing to suppress.
    @unittest.skipUnless(shutil.which("rustfmt"), "rustfmt not installed")
    def test_rustfmt_reflow_of_a_long_arm_keeps_the_check_on(self):
        # THE property that actually matters. Measured 2026-07-26 with
        # rustfmt 1.9.0 --edition 2021 by binary search: at indent 8 with
        # a 3-digit key, a same-line arm whose display value is >= 71
        # chars is rewritten into `260 => {` / `"...".to_string()` / `}`.
        # 2.44% of the 19,796 distinct display strings in
        # Image/ExifTool/*.pm are that long, so before this fix the byte
        # check silently switched off for long values with no adversarial
        # intent required -- and `cargo fmt` runs on every sweep branch.
        long_value = "Fabricated " + "Lens Name " * 6  # 71 chars
        self.assertGreaterEqual(len(long_value), 71)
        source = (
            "pub fn lens_type(v: u16) -> String {\n"
            "    match v {\n"
            f'        260 => "{long_value}".to_string(),\n'
            '        _ => "Unknown".to_string(),\n'
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "lens.rs"
            path.write_text(source)
            subprocess.run(  # nosec B603 -- list-argv, no shell; the only
                # interpolated element is a tempdir path this test made.
                ["rustfmt", "--edition", "2021", str(path)],
                check=True, capture_output=True,
            )
            formatted = path.read_text()
        # rustfmt really did produce the block shape ...
        self.assertIn("260 => {", formatted)
        diff = (
            "diff --git a/src/canon/lens.rs b/src/canon/lens.rs\n"
            "@@ -1,6 +1,8 @@ pub fn lens_type(v: u16) -> String {\n"
            + "".join(f"+{line}\n" for line in formatted.splitlines())
        )
        values, _ = extract_added_map_values(diff)
        # ... and the value is still checked.
        self.assertIn(long_value, values)

    def test_block_arm_fabrication_is_flagged_end_to_end(self):
        rust = (
            "pub fn lens_type(v: u16) -> String {\n"
            "    match v {\n"
            "        1 => {\n"
            '            "Economy".to_string()\n'
            "        }\n"
            "        260 => {\n"
            '            "Fabricated Lens That Is Not In Any Perl Module".to_string()\n'
            "        }\n"
            '        _ => "Unknown".to_string(),\n'
            "    }\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            sha = commit_fix(repo, {"src/canon/lens.rs": rust}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn(
            "printconv-mismatch:Fabricated Lens That Is Not In Any Perl Module",
            result["flags"])
        self.assertFalse(result["ok"])
        # The genuine arm in the same block, written in the same shape,
        # is byte-verified rather than merely ignored.
        self.assertNotIn("printconv-mismatch:Economy", result["flags"])

    def test_stringless_block_arm_bodies_stay_clean(self):
        # The false-quarantine class the `=>` rule was loosened for in
        # POLICY_VERSION 4 must not come back through the block door.
        diff = (
            "diff --git a/src/raw/metadata.rs b/src/raw/metadata.rs\n"
            "@@ -10,3 +10,14 @@ fn read_header(bytes: &[u8]) -> Result<()> {\n"
            "+        ByteOrder::BigEndian => {\n"
            "+            u16::from_be_bytes([bytes[0], bytes[1]])\n"
            "+        }\n"
            "+        Err(_) => {\n"
            "+            return;\n"
            "+        }\n"
            "+        _ => {\n"
            "+            continue;\n"
            "+        }\n"
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, [])
        self.assertEqual(unverifiable, [])

    def test_format_template_inside_a_block_arm_is_not_a_value(self):
        # A template is not a display value, and inside a BODY it is not
        # raised as printconv-unverifiable either: measured 2026-07-26
        # over all 2,348 worker diffs, doing so newly blocks 227 of them
        # against the 100 that carry the flag today, and a `{}` template
        # can never be a fabricated ExifTool string. Quarantined head
        # 4a71eb0a4b72 -- a real CR2 LensInfo fix whose whole arm body is
        # `format!("{:.1}", ...)` -- is one of the 227.
        diff = (
            "diff --git a/src/x.rs b/src/x.rs\n"
            "@@ -1,2 +1,5 @@ fn describe(other: u32) -> String {\n"
            "+        other => {\n"
            '+            format!("Unknown({})", other)\n'
            "+        }\n"
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, [])
        self.assertEqual(unverifiable, [])

        # The SAME-LINE arm keeps its unverifiable flag: that rule is
        # about an arm whose entire value is computed, and nothing here
        # loosens it.
        same_line = (
            "diff --git a/src/x.rs b/src/x.rs\n"
            "@@ -1,2 +1,3 @@ fn describe(other: u32) -> String {\n"
            '+        other => format!("Unknown({})", other),\n'
        )
        values, unverifiable = extract_added_map_values(same_line)
        self.assertEqual(values, [])
        self.assertEqual(len(unverifiable), 1)

    def test_only_the_arms_own_value_is_taken_from_its_body(self):
        # Everything else in a body is ordinary code: separators, byte
        # magic, key prefixes and tag names. Extracting those re-imported
        # the false-quarantine class POLICY_VERSION 3 and 4 were spent
        # removing (334 of 2,348 real worker diffs, measured 2026-07-26).
        diff = (
            "diff --git a/src/parsers/raw/dng.rs b/src/parsers/raw/dng.rs\n"
            "@@ -10,3 +10,10 @@ fn decode(tag: u16) -> Option<String> {\n"
            "+        0xC61A => {\n"
            '+            let key = format!("EXIF:{}", "BlackLevel");\n'
            '+            let joined = parts.join(", ");\n'
            '+            md.insert(key, "Fabricated Body Value".to_string());\n'
            '+            "Fabricated Arm Value".to_string()\n'
            "+        }\n"
        )
        values, _ = extract_added_map_values(diff)
        # The arm's own value, and the .insert() value (its own rule) --
        # but not the key prefix, the tag name or the separator.
        self.assertIn("Fabricated Arm Value", values)
        self.assertIn("Fabricated Body Value", values)
        self.assertNotIn("BlackLevel", values)
        self.assertNotIn(", ", values)

    def test_multiline_assert_inside_a_block_arm_is_ignored(self):
        diff = (
            "diff --git a/src/thermal.rs b/src/thermal.rs\n"
            "@@ -200,3 +200,9 @@ mod tests {\n"
            "+        0 => {\n"
            "+            assert_eq!(\n"
            '+                metadata.get_string("APP12:Flash"),\n'
            '+                Some("Off"),\n'
            "+                \"Flash=0 should be PrintConv'd to Off\"\n"
            "+            );\n"
            "+        }\n"
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, [])

    def test_arm_body_tracking_closes_and_does_not_leak(self):
        # The body skip/extract window must CLOSE on the matching brace,
        # and must never survive a hunk or file boundary -- the same
        # invariant the bracket-depth tracker already has.
        diff = (
            "diff --git a/src/canon/lens.rs b/src/canon/lens.rs\n"
            "@@ -20,6 +20,7 @@ pub fn lens_type(v: u16) -> String {\n"
            "+        260 => {\n"
            '+            "Fabricated Inside".to_string()\n'
            "+        }\n"
            "+    }\n"
            '+    let unrelated = "Not A Map Value";\n'
            "diff --git a/src/other.rs b/src/other.rs\n"
            "@@ -1,2 +1,3 @@ fn other() {\n"
            '+    let also_unrelated = "Still Not A Map Value";\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Fabricated Inside"])

    # -- the repo's own table shapes ------------------------------------

    def test_const_decoder_table_values_are_extracted(self):
        # `const_decoder!` is oxidex's canonical PrintConv table macro --
        # 338 uses across 46 files in src/ (counted 2026-07-26) -- and the
        # module docstring already claimed to cover "const_decoder-style
        # tables". It did not: _CONST_STATIC_RE's \b(?:const|static)\b
        # cannot match `const_decoder!` (the underscore kills the word
        # boundary) and `(12, "...")` is neither a `=>` line nor a bare
        # string element. Real fleet output: 17 distinct worker diffs add
        # 21 distinct multi-word display strings in this shape.
        diff = (
            "diff --git a/src/parsers/tiff/makernotes/canon.rs "
            "b/src/parsers/tiff/makernotes/canon.rs\n"
            "@@ -40,6 +40,12 @@\n"
            #
            # The entries deliberately SHARE a line and one key is
            # parenthesised. One tuple per line let this test pass with
            # _TABLE_MACRO_RE replaced by a never-matching pattern --
            # _BARE_TUPLE_ELEMENT_RE carried it alone, so the macro
            # recognition that is the headline of this fix was unpinned.
            # Verified 2026-07-26 by mutation.
            "+const_decoder!(\n"
            "+    pub ASPECT_RATIO,\n"
            "+    i32,\n"
            '+    [(0, "3:2"), (mask(12), "Totally Fabricated Crop Mode")]\n'
            "+);\n"
        )
        values, _ = extract_added_map_values(diff)
        self.assertIn("Totally Fabricated Crop Mode", values)

    def test_single_line_const_decoder_table_values_are_extracted(self):
        diff = (
            "diff --git a/src/parsers/tiff/makernotes/canon.rs "
            "b/src/parsers/tiff/makernotes/canon.rs\n"
            "@@ -40,6 +40,7 @@\n"
            '+const_decoder!(pub AF_MICRO_ADJ, i16, [(1, "Fabricated Adjust Mode")]);\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Fabricated Adjust Mode"])

    def test_mid_table_tuple_element_is_extracted(self):
        # A value inserted into the MIDDLE of an existing decoder table:
        # the declaring `[` is outside the hunk, exactly like the
        # bare-string case _BARE_STRING_ELEMENT_RE exists for.
        diff = (
            "diff --git a/src/parsers/tiff/makernotes/canon.rs "
            "b/src/parsers/tiff/makernotes/canon.rs\n"
            "@@ -140,6 +140,7 @@\n"
            '         (11, "1:1"),\n'
            '+        (12, "Fabricated Crop Mode"),\n'
            '         (13, "16:9"),\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Fabricated Crop Mode"])

    def test_for_loop_inline_table_values_are_extracted(self):
        # `for (bit, name) in [(29, "Main 10"), ...]` -- a real QuickTime
        # HEVC fix used this shape and its whole diff extracted nothing.
        #
        # The tuples MUST share a line here. Written one-per-line this
        # test passed with _INLINE_TABLE_RE replaced by a never-matching
        # pattern, because _BARE_TUPLE_ELEMENT_RE satisfied it on its own
        # -- so it pinned nothing of the rule it is named for. Verified
        # 2026-07-26 by mutation: with the rule neutered, this now fails.
        diff = (
            "diff --git a/src/parsers/quicktime/hevc.rs "
            "b/src/parsers/quicktime/hevc.rs\n"
            "@@ -10,6 +10,8 @@ fn profiles(flags: u32) -> Vec<String> {\n"
            '+    for (bit, name) in [(29, "Main 10"), (30, "Fabricated Profile Name")] {\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertIn("Fabricated Profile Name", values)
        self.assertIn("Main 10", values)

    def test_map_insert_value_is_extracted(self):
        diff = (
            "diff --git a/src/parsers/raw/rw2.rs b/src/parsers/raw/rw2.rs\n"
            "@@ -10,6 +10,8 @@ fn quality_map() -> HashMap<u16, &'static str> {\n"
            '+    map.insert(1u16, "Economy");\n'
            '+    map.insert(2u16, "Fabricated Fine Detail");\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Economy", "Fabricated Fine Detail"])

    def test_insert_keyed_by_a_metadata_key_stays_clean(self):
        # The overwhelmingly common `.insert(...)` shape in src/ (1,029
        # sites counted 2026-07-26) puts an oxidex metadata KEY first and
        # a runtime value second -- no display string to check, and the
        # key itself is an identifier, not a PrintConv value.
        diff = (
            "diff --git a/src/parsers/jpeg/exif.rs b/src/parsers/jpeg/exif.rs\n"
            "@@ -10,6 +10,9 @@ fn emit(md: &mut Metadata) {\n"
            '+    md.insert("EXIF:Make", make_value);\n'
            '+    md.insert("EXIF:Model", model.to_string());\n'
            '+    md.insert(format!("ICC_Profile:{}", tag), value);\n'
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, [])
        self.assertEqual(unverifiable, [])

    def test_perl_ref_is_required_when_the_values_live_in_a_decoder_table(self):
        # Second-order effect of the blind spot: check_trailers is called
        # with require_perl_ref=bool(extract_added_map_values(diff)[0]),
        # so a diff whose only display strings sat in a const_decoder!
        # table was not even required to cite the Perl evidence a human
        # would need to audit it.
        rust = (
            "const_decoder!(\n"
            "    pub ASPECT_RATIO,\n"
            "    i32,\n"
            "    [\n"
            '        (0, "Economy"),\n'
            '        (12, "Fine"),\n'
            "    ]\n"
            ");\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(Path(tmp) / "perl", PERL_QUALITY)
            sha = commit_fix(
                repo, {"src/canon/aspect.rs": rust}, full_trailers(**{"Perl-Ref": None})
            )
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("missing-trailer:Perl-Ref", result["flags"])
        self.assertFalse(result["ok"])

    # -- trailing comments on bare table elements ------------------------

    def test_trailing_comments_do_not_hide_a_mid_table_value(self):
        # src/parsers/icc/registries.rs writes its indexed display-value
        # tables as `"F2",             // 4`. The `$`-anchored bare-string
        # rule matched none of those, so 3 of the 4 ICC tables the
        # POLICY_VERSION 5 comment claims to protect were unprotected in
        # their own house style (measured 2026-07-26: only tables whose
        # declaring line lands inside the 3-line diff context survived).
        for element in (
            '+    "FAB COMMENT VALUE", // 42',
            '+    "FAB COMMENT VALUE", /* 42 */',
            '+    "FAB COMMENT VALUE",            // 4b',
            '+    "FAB COMMENT VALUE" // trailing, no comma',
        ):
            with self.subTest(element=element):
                diff = (
                    "diff --git a/src/parsers/icc/registries.rs "
                    "b/src/parsers/icc/registries.rs\n"
                    "@@ -539,6 +539,7 @@ pub static ILLUMINANT_TYPES: &[&str] = &[\n"
                    '     "F2",             // 4\n'
                    f"{element}\n"
                    '     "F7",             // 5\n'
                )
                values, _ = extract_added_map_values(diff)
                self.assertEqual(values, ["FAB COMMENT VALUE"])

    def test_a_url_inside_a_string_is_not_mistaken_for_a_comment(self):
        # Why the comment strip is a scanner and not a regex: a `//`
        # inside a string literal is not a comment.
        diff = (
            "diff --git a/src/parsers/icc/registries.rs "
            "b/src/parsers/icc/registries.rs\n"
            "@@ -539,6 +539,7 @@ pub static ILLUMINANT_TYPES: &[&str] = &[\n"
            '+    "http://x.example//y",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["http://x.example//y"])

    def test_the_comment_scanner_handles_the_cases_a_regex_would_not(self):
        strip = validate_fix_commit.strip_trailing_comment
        self.assertEqual(strip('    "FAB", // 42'), '    "FAB", ')
        self.assertEqual(strip('    "FAB", /* 42 */'), '    "FAB", ')
        # `//` and `/*` inside a literal are literal text, not comments.
        self.assertEqual(strip('    "http://x//y",'), '    "http://x//y",')
        self.assertEqual(strip('    "a /* b */ c",'), '    "a /* b */ c",')
        # An escaped quote does not end the literal early.
        self.assertEqual(strip('    "say \\" // no",'), '    "say \\" // no",')
        # An unterminated block comment swallows the rest of the line.
        self.assertEqual(strip('    "FAB", /* 42'), '    "FAB", ')
        # A comment-only line becomes empty, so no rule can match it.
        self.assertEqual(strip("    // 42").strip(), "")

    def test_a_tableless_decoder_macro_does_not_pend(self):
        # `const_decoder!(` pends until its `[` arrives on a later line
        # (rustfmt's multi-line form). A macro call that CLOSES on its own
        # line without a table must NOT stay pending, or the next `[`
        # anywhere in the hunk gets read as that table's body.
        tail = validate_fix_commit._table_literal_tail
        self.assertEqual(tail("const_decoder!(", False), (None, True))
        self.assertEqual(tail("    register_decoder!(ASPECT_RATIO, i32);", False),
                         (None, False))
        # While pending, the `[` line opens the table; a macro argument
        # line keeps pending without opening one.
        self.assertEqual(tail("    pub ASPECT_RATIO,", True), (None, True))
        self.assertEqual(tail("    [", True), ("    [", False))

    def test_a_string_that_only_exists_inside_a_comment_is_not_a_value(self):
        diff = (
            "diff --git a/src/parsers/icc/registries.rs "
            "b/src/parsers/icc/registries.rs\n"
            "@@ -539,6 +539,8 @@ pub static ILLUMINANT_TYPES: &[&str] = &[\n"
            '+    "Real Value", // see "Commented Out Value"\n'
            '+    // "Fully Commented Value",\n'
        )
        values, _ = extract_added_map_values(diff)
        self.assertEqual(values, ["Real Value"])

    def test_icc_house_style_insert_is_flagged_end_to_end(self):
        # The real table, the real style, the real consumer: tags.rs does
        # `ILLUMINANT_TYPES.get(illum_type as usize)`, so a fabricated
        # element is a user-visible wrong metadata value AND it shifts
        # every later index.
        base = (
            "pub static ILLUMINANT_TYPES: &[&str] = &[\n"
            '    "Unknown",        // 0 - not used\n'
            '    "D50",            // 1\n'
            '    "D65",            // 2\n'
            '    "D93",            // 3\n'
            '    "F2",             // 4\n'
            '    "D55",            // 5\n'
            '    "A",              // 6\n'
            '    "Equi-Power (E)", // 7\n'
            '    "F8",             // 8\n'
            "];\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            repo = make_repo(tmp)
            perl_lib = write_perl_module(
                Path(tmp) / "perl", "%illum = (1 => 'D50', 2 => 'D65');\n"
            )
            commit_fix(repo, {"src/parsers/icc/registries.rs": base},
                       full_trailers(), subject="base: illuminants")
            fabricated = base.replace(
                '    "D55",            // 5\n',
                '    "D77",            // 5b\n'
                '    "D55",            // 5\n')
            sha = commit_fix(
                repo, {"src/parsers/icc/registries.rs": fabricated}, full_trailers())
            result = validate_commit(sha, repo, perl_lib=perl_lib)
        self.assertIn("printconv-mismatch:D77", result["flags"])
        self.assertFalse(result["ok"])

    # -- the human-routing gate shares this extractor --------------------

    def test_the_judgment_queue_also_sees_these_shapes_now(self):
        # overlord_sweep.classify_for_judgment_queue calls THIS extractor
        # for its only PrintConv reason, so every shape above defeated the
        # machine gate and the human-routing gate simultaneously: the
        # commit shipped as machine_accepted with no review at all.
        # Imported lazily so the rest of this file does not depend on the
        # sweep module (verified side-effect-free on import 2026-07-26:
        # it writes nothing under ~/.oxidex at import time).
        import overlord_sweep

        shapes = {
            "block arm": (
                "pub fn lens(v: u16) -> String {\n"
                "    match v {\n"
                "        260 => {\n"
                '            "Fabricated Block Value".to_string()\n'
                "        }\n"
                "    }\n"
                "}\n"
            ),
            "insert": (
                "pub fn build(map: &mut HashMap<u16, &'static str>) {\n"
                '    map.insert(9, "Fabricated Insert Value");\n'
                "}\n"
            ),
            "decoder table": (
                "const_decoder!(\n"
                "    pub ASPECT_RATIO,\n"
                "    i32,\n"
                "    [\n"
                '        (12, "Fabricated Table Value"),\n'
                "    ]\n"
                ");\n"
            ),
        }
        for name, rust in shapes.items():
            with self.subTest(shape=name), tempfile.TemporaryDirectory() as tmp:
                repo = make_repo(tmp)
                sha = commit_fix(repo, {"src/canon/x.rs": rust}, full_trailers())
                reasons = overlord_sweep.classify_for_judgment_queue(
                    sha, repo, validate_fix_commit.run_git)
                self.assertIn("touches a value-map/PrintConv-like table", reasons)

    # -- the docstring must describe what the code does ------------------

    def test_the_docstring_does_not_promise_unimplemented_behaviour(self):
        # The module docstring promised that "computed right-hand sides
        # (format!/sprintf-style, function calls, match-arm blocks)" are
        # reported printconv-unverifiable. Only the macro half was ever
        # implemented: a plain call's quoted argument is EXTRACTED and
        # byte-checked, and match-arm blocks are now extracted too (that
        # is the point of this change). A gate's docstring claiming a
        # protection it does not have is worse than no docstring.
        doc = " ".join((validate_fix_commit.__doc__ or "").split())
        # The exact old promise, whitespace-normalised.
        self.assertNotIn(
            "computed right-hand sides (format!/sprintf-style, function "
            "calls, match-arm blocks)", doc)
        # And it now names the shapes it really does extract.
        self.assertIn("block-bodied arms", doc)

        # ... and here is the behaviour the old text mis-described: the
        # quoted argument of a plain function call IS checked. Keeping it
        # checked is deliberate -- `Some("Centered".to_string())`,
        # `String::from("sRGB")` and `.insert(k, "Fine")` are all
        # single-shape function calls carrying genuine display values,
        # and a "skip strings that are call arguments" rule would re-open
        # exactly the `.insert(k, "...")` hole closed above.
        diff = (
            "diff --git a/src/parsers/raw/cr2.rs b/src/parsers/raw/cr2.rs\n"
            "@@ -10,3 +10,4 @@ fn tag_name(tag_id: u16) -> String {\n"
            '+        (_, 0x080a) => lookup_tag_name(tag_id, "EXIF"),\n'
        )
        values, unverifiable = extract_added_map_values(diff)
        self.assertEqual(values, ["EXIF"])
        self.assertEqual(unverifiable, [])

    def test_policy_version_was_bumped_for_the_new_extraction_rules(self):
        # The header mandates a bump for "any change to what
        # extract_added_map_values extracts", in either direction: it is
        # what lets squad_merge_loop re-offer already-quarantined heads,
        # and what tells a human reading quarantine.jsonl which ruleset
        # produced a verdict.
        self.assertGreaterEqual(validate_fix_commit.POLICY_VERSION, 6)


if __name__ == "__main__":
    unittest.main()
